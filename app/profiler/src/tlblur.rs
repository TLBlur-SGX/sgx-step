use std::{
    collections::{HashMap, HashSet, VecDeque},
    error::Error,
    ffi::c_void,
    fmt::Display,
    io::Read,
};

use clap::{Parser, ValueEnum};
use sgx_profiler::{
    create_dumper, create_enclave, create_trap_handler,
    dump::{RSet, VCDDumper, VCDEntry},
    run_profiler,
    sgx_step::memory::EnclaveMemory,
    PageAccess, PageTable, ProfilerLibrary,
};
use sgx_step::{sgx_step_sys::PAGE_SIZE_4KiB, EnclaveRef};

pub struct PageTableObservations {
    state: HashMap<usize, PageAccess>,
}

impl PageTableObservations {
    pub fn new() -> Self {
        Self {
            state: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.state.clear()
    }

    pub fn update<'a>(&mut self, pages: impl Iterator<Item = &'a PageAccess>) {
        for page in pages {
            self.state
                .entry(page.page)
                .and_modify(|e| *e = e.union(page))
                .or_insert(page.to_owned());
        }
    }

    fn iter<'a>(&'a self) -> impl Iterator<Item = &'a PageAccess> {
        self.state.values()
    }
}

pub struct PAM {
    pam_enclave_mem: EnclaveMemory,
    pam_counter_enclave_mem: EnclaveMemory,
    pam_buffer: Vec<u64>,
    pam_active: Vec<PageAccess>,
    pam_counter: u64,
}

impl PAM {
    pub fn new(
        pam_address: *const c_void,
        pam_counter_address: *const c_void,
        pam_size: usize,
        pws_size: usize,
    ) -> Self {
        Self {
            pam_enclave_mem: EnclaveMemory::new(pam_address as usize),
            pam_counter_enclave_mem: EnclaveMemory::new(pam_counter_address as usize),
            pam_buffer: vec![0; pam_size],
            pam_active: vec![PageAccess::default(); pws_size],
            pam_counter: 0,
        }
    }

    fn get_pam(&self) -> impl Iterator<Item = &PageAccess> {
        self.pam_active.iter()
    }

    pub fn update_pam(&mut self) {
        let old_counter = self.pam_counter;

        // Read the new PAM counter from enclave memory
        let mut buf: [u8; 8] = [0; 8];
        self.pam_counter_enclave_mem.read(&mut buf).unwrap();
        let new_counter = u64::from_le_bytes(buf);

        // If the counter changed compared to previous step of execution,
        // then our local view of the PAM must be updated to match the one in enclave memory.
        //
        // In contrast to the representation of the PAM in enclave memory,
        // PAM stored by the profiler more closely aligns with a real TLB, as it
        // only contains the N most recent pages.
        //
        // It should match the behavior of the PAM, but it should not try to mimic the real TLB.
        //
        // NOTE: an assumption is made that at the time the counter is incremented, the PAM is
        // already updated as well. We use the PAM global counter as a way to signal the
        // profiler of a PAM update, to avoid having to walk through the entire PAM each step.
        // This requires the instrumentation to be written in a specific way.
        if old_counter != new_counter {
            // println!("counter: {}", new_counter);
            // Read the PAM from enclave memory
            self.pam_enclave_mem
                .read(unsafe { std::mem::transmute(self.pam_buffer.as_mut_slice()) })
                .unwrap();

            let mut found = false;
            for (page, &value) in self.pam_buffer.iter().enumerate() {
                // Only update this entry in profiler PAM if it was recently updated.
                if value >= new_counter - 1 && value > 0 {
                    self.pam_counter = new_counter;
                    // Only update if not already in profiler PAM
                    found = true;
                    if self
                        .pam_active
                        .iter()
                        .find(|p| p.page == (page as usize))
                        .is_none()
                    {
                        // println!("new entry in PAM: {}", page);
                        // Find the least recently used entry to evict according
                        // to the state of the PAM
                        if let Some((index, _)) =
                            self.pam_active.iter().enumerate().min_by_key(|&(_, &p)| {
                                if p.page == 0 {
                                    0
                                } else {
                                    self.pam_buffer[p.page]
                                }
                            })
                        {
                            // println!("replaced an entry");
                            // Replace the entry
                            self.pam_active[index].page = page;

                            // The real prefetcher can't do this,
                            // but we can in the profiler because we don't care about
                            // the permissions of pages.
                            //
                            // The real prefetcher would instead use the maximum
                            // allowed permissions, we should be equivalent.
                            self.pam_active[index].read = true;
                            self.pam_active[index].write = true;
                            self.pam_active[index].execute = true;
                        }
                    } else {
                        // println!("already in PAM");
                    }
                }
            }
            if !found && new_counter - old_counter > 1 {
                // println!("Warning: PAM counter incremented, but new entry not found!");
            }
        }
    }
}

unsafe impl Sync for PAM {}
unsafe impl Send for PAM {}

#[derive(Debug, Clone)]
pub struct TLBEntry {
    page: PageAccess,
    valid: bool,
}

#[derive(Debug, Clone)]
pub struct Set {
    ways: VecDeque<TLBEntry>,
    capacity: usize,
}

impl Set {
    pub fn new(capacity: usize) -> Self {
        Set {
            ways: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn lookup(&self, page: &PageAccess) -> bool {
        for entry in &self.ways {
            if entry.page.covers(page) && entry.valid {
                return true;
            }
        }
        false
    }

    pub fn insert(&mut self, page: PageAccess) {
        // Check if the page is already in the set
        if let Some(pos) = self
            .ways
            .iter()
            .position(|entry| entry.page.covers(&page) && entry.valid)
        {
            // Move the found entry to the back (most recently used)
            let entry = self.ways.remove(pos).unwrap();
            self.ways.push_back(entry);
        } else {
            // Insert new entry, evicting the least recently used if necessary
            if self.ways.len() == self.capacity {
                self.ways.pop_front(); // Evict the least recently used (LRU) entry
            }
            self.ways.push_back(TLBEntry { page, valid: true });
        }
    }

    pub fn invalidate(&mut self, page: &PageAccess) {
        for entry in &mut self.ways {
            if entry.page.covers(page) {
                entry.valid = false;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum HardwareTLBType {
    Perfect,
    SetAssociative,
}

#[derive(Debug, Clone, Copy)]
pub enum HardwareTLBConfig {
    Perfect,
    SetAssociative {
        num_sets: usize,
        ways_per_set: usize,
    },
}

#[derive(Debug, Clone)]
pub enum HardwareTLB {
    Perfect(HashSet<PageAccess>),
    SetAssociative {
        sets: Vec<Set>,
        num_sets: usize,
        ways_per_set: usize,
    },
}

impl HardwareTLB {
    pub fn flush(&mut self) {
        match self {
            Self::Perfect(ref mut pages) => pages.clear(),
            Self::SetAssociative { sets, .. } => {
                for set in sets {
                    set.ways.clear();
                }
            }
        }
    }

    pub fn update<'a>(&mut self, pages: impl Iterator<Item = &'a PageAccess>) {
        match self {
            Self::Perfect(ref mut tlb) => {
                // "perfect" fully-associative hardware TLB with infinite size
                for page in pages {
                    tlb.insert(page.to_owned());
                }
            }
            Self::SetAssociative { sets, num_sets, .. } => {
                for page in pages {
                    let set_index = Self::get_set_index(page, *num_sets);
                    sets[set_index].insert(page.to_owned());
                }
            }
        }
    }

    pub fn test(&self, page: &PageAccess) -> bool {
        match self {
            Self::Perfect(pages) => pages.iter().any(|p| p.covers(page)),
            Self::SetAssociative { sets, num_sets, .. } => {
                let set_index = Self::get_set_index(page, *num_sets);
                sets[set_index].lookup(page)
            }
        }
    }

    /// Use for debugging purposes only
    pub fn iter(&self) -> impl Iterator<Item = &PageAccess> {
        match self {
            Self::Perfect(pages) => pages.iter(),
            Self::SetAssociative { .. } => todo!(),
        }
    }

    fn get_set_index(page: &PageAccess, num_sets: usize) -> usize {
        (page.page as usize) % num_sets
    }
}

impl From<HardwareTLBConfig> for HardwareTLB {
    fn from(value: HardwareTLBConfig) -> Self {
        match value {
            HardwareTLBConfig::Perfect => Self::Perfect(HashSet::new()),
            HardwareTLBConfig::SetAssociative {
                num_sets,
                ways_per_set,
            } => Self::SetAssociative {
                sets: (0..num_sets).map(|_| Set::new(ways_per_set)).collect(),
                num_sets,
                ways_per_set,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InterruptPattern {
    DebugSingleStep,
    SingleStep,
    PageFault,
    Stealthy,
}

impl Display for InterruptPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DebugSingleStep => "debug-single-step",
            Self::SingleStep => "single-step",
            Self::PageFault => "page-fault",
            Self::Stealthy => "stealthy",
        })
    }
}

#[derive(Debug, Clone)]
enum Attacker {
    DebugSingleStep,
    SingleStep,
    PageFault {
        live_pages: Vec<usize>,
        observe_ptes: bool,
    },
    Stealthy,
}

impl From<InterruptPattern> for Attacker {
    fn from(value: InterruptPattern) -> Self {
        match value {
            InterruptPattern::DebugSingleStep => Attacker::DebugSingleStep,
            InterruptPattern::SingleStep => Attacker::SingleStep,
            InterruptPattern::PageFault => Attacker::PageFault {
                live_pages: Vec::new(),
                observe_ptes: true,
            },
            InterruptPattern::Stealthy => Attacker::Stealthy,
        }
    }
}

impl Display for Attacker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DebugSingleStep => "debug-single-step",
            Self::SingleStep => "single-step",
            Self::PageFault { .. } => "page-fault",
            Self::Stealthy => "stealthy",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanObserve {
    Always,
    Interrupt,
}

impl Attacker {
    /// Given the behaviour of the attacker and the state of the HW TLB and page table,
    /// would the attacker be able to interrupt at this point.
    fn can_trigger_interrupt(&self, page_table: &PageTable, hw_tlb: &HardwareTLB) -> bool {
        match self {
            Attacker::DebugSingleStep => {
                // A single-stepping adversary can always interrupt, regardless of the state of
                // the hardware TLB.
                //
                // This is an unrealistic attacker model, as our defense prohibits such behavior
                //
                // An attack like this would require an enclave in debug mode with trap interrupts
                true
            }
            Attacker::SingleStep => {
                // We assume that this attacker can interrupt if there is some page accessed
                // (change in PTE A bit) that was not in the hardware TLB.
                //
                // This is essentially the SGX-Step attacker
                page_table.get_accessed_pages(|p| !hw_tlb.test(p)).count() > 0
            }
            Attacker::PageFault { live_pages, .. } => {
                // The page fault attacker is like the single-stepping attacker, but has a
                // set of live pages that are mapped. An interrupt can only be triggered
                // by this attacker if there is a page that is not in the hardware TLB
                // *and* not in the set of pages that the attacker made accessible.

                page_table
                    .get_accessed_pages(|p| !hw_tlb.test(p))
                    .any(|p| !live_pages.contains(&p.page))
            }
            Attacker::Stealthy => {
                // The stealthy attacker only observes changes to PTE bits, but never interrupts
                false
            }
        }
    }

    fn observe<'d>(
        &self,
        entry: &mut VCDEntry<'d, RSet>,
        page_table: &PageTable,
        hw_tlb: &HardwareTLB,
        observations: &mut PageTableObservations,
    ) {
        match self {
            Attacker::PageFault {
                ref live_pages,
                observe_ptes: false,
            } => entry.write_page_accesses(
                page_table
                    .get_accessed_pages(|p| !hw_tlb.test(p))
                    .filter(|p| !live_pages.contains(&p.page)),
            ),
            _ => entry.write_page_accesses(observations.iter()),
        };
    }

    fn can_observe(&self) -> CanObserve {
        match self {
            // Stealthy attacker sees everything without interrupts
            Attacker::Stealthy => CanObserve::Always,
            // Other attackers only observe on interrupt
            _ => CanObserve::Interrupt,
        }
    }

    fn handle_step(&mut self, observations: &mut PageTableObservations) {
        match self {
            Attacker::Stealthy => observations.clear(),
            _ => {}
        }
    }

    fn handle_interrupt(
        &mut self,
        page_table: &PageTable,
        observations: &mut PageTableObservations,
    ) {
        match self {
            Attacker::PageFault {
                ref mut live_pages, ..
            } => {
                // This attacker maps the pages that are necessary for the current instruction
                // to execute. It can then not trigger page faults on those pages, so
                // we record it in the live pages set to remember the current capabilities of
                // the attacker.
                live_pages.clear();
                for page in page_table.get_all_accessed_pages() {
                    live_pages.push(page.page);
                }
                observations.clear();
            }
            Attacker::Stealthy => {}
            _ => {
                // All other attackers clear PTE bits as often as possible
                observations.clear();
            }
        }
    }
}

/// SGX tlblur simulator
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// A shared object that provides the profiler_setup and profiler_run functions
    #[arg(long)]
    so: String,

    /// An SGX binary that will be created by the profiler
    #[arg(short, long)]
    enclave: String,

    /// Output VCD file
    #[arg(short = 'o', long = "output")]
    trace_output: String,

    #[arg(long)]
    debug_pam: Option<String>,

    #[arg(long)]
    debug_sim_hwtlb: Option<String>,

    /// Arguments to pass to the profiler_run function
    #[arg(long, value_parser, num_args = 1.., value_delimiter = ' ')]
    args: Vec<String>,

    /// Write erip to VCD output
    #[arg(long = "erip")]
    write_erip: bool,

    /// Size of the software TLB to simulate
    #[arg(long, default_value_t = 10)]
    pws_size: usize,

    #[arg(long = "irq-pat", short = 'p', default_value_t = InterruptPattern::SingleStep)]
    interrupt_pattern: InterruptPattern,

    #[arg(long = "observe-ptes", default_value_t = true)]
    observe_ptes: bool,

    #[arg(long = "hw-tlb")]
    hardware_tlb: HardwareTLBType,

    #[arg(long = "sets", default_value_t = 4)]
    num_sets: usize,

    #[arg(long = "ways", default_value_t = 2)]
    ways_per_set: usize,

    #[arg(long)]
    no_prefetch: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let enclave = create_enclave(&args.enclave)?;

    let library = unsafe { libloading::Library::new(&args.so)? };

    let pam_address = enclave.symbol_address("__tlblur_pam")? as u64;
    let pam_counter_address = enclave.symbol_address("__tlblur_counter")? as u64;
    let pam_update_code_address = enclave.symbol_address("tlblur_pam_update")? as u64;

    let mut dumper: VCDDumper<RSet> = create_dumper(&enclave, &args.trace_output);
    let mut pam_dumper: Option<VCDDumper<RSet>> =
        args.debug_pam.map(|f| create_dumper(&enclave, f));
    let mut hwtlb_dumper: Option<VCDDumper<RSet>> =
        args.debug_sim_hwtlb.map(|f| create_dumper(&enclave, f));
    let mut page_table = PageTable::new(&enclave);
    let num_pages = page_table.page_table_map.len();
    let mut pam = PAM::new(
        pam_address as *mut c_void,
        pam_counter_address as *mut c_void,
        num_pages * 8,
        args.pws_size,
    );
    let write_erip = args.write_erip;
    let no_prefetch = args.no_prefetch;
    let mut attacker: Attacker = args.interrupt_pattern.into();
    if let Attacker::PageFault {
        ref mut observe_ptes,
        ..
    } = attacker
    {
        *observe_ptes = args.observe_ptes;
    }
    let mut hw_tlb = HardwareTLB::from(match args.hardware_tlb {
        HardwareTLBType::Perfect => HardwareTLBConfig::Perfect,
        HardwareTLBType::SetAssociative => HardwareTLBConfig::SetAssociative {
            num_sets: args.num_sets,
            ways_per_set: args.ways_per_set,
        },
    });
    let mut pte_observations = PageTableObservations::new();

    // Don't do this, this is a hacky way to get around Rust's aliasing rules
    let enclave_ref = unsafe { EnclaveRef::from_raw(enclave.id()) };

    let mut first_run = true;

    create_trap_handler(move || {
        // Update the local PAM to match the one in the instrumented enclave
        pam.update_pam();

        // Need to "prime" the page table on the first interrupt
        // to get accurate measurements.
        if first_run {
            first_run = false;
            page_table.clear_all_ad_bits();
            return;
        }

        pam_dumper.as_mut().map(|d| {
            d.next_step(|entry| {
                if write_erip {
                    entry.write_erip();
                }

                entry.write_page_accesses(pam.get_pam());
            })
        });

        hwtlb_dumper.as_mut().map(|d| {
            d.next_step(|entry| {
                if write_erip {
                    entry.write_erip();
                }

                entry.write_page_accesses(hw_tlb.iter());
            })
        });

        // Check which pages were accessed
        page_table.update_page_accesses();

        // This is the effect on the real page table, which we simulate,
        // because the real page table is used to trace page accesses of each instruction
        pte_observations.update(page_table.get_accessed_pages(|p| !hw_tlb.test(p)));

        let can_observe = attacker.can_observe();
        let can_trigger_interrupt = attacker.can_trigger_interrupt(&page_table, &hw_tlb);

        // Only write observations to the VCD trace if the attacker can observe
        if can_observe == CanObserve::Always
            || can_trigger_interrupt && can_observe == CanObserve::Interrupt
        {
            // Write to VCD trace
            dumper.next_step(|entry| {
                if write_erip {
                    entry.write_erip();
                }

                // An attacker can only observe accesses to pages not in the hardware TLB
                // entry.write_page_accesses(page_table.get_accessed_pages(|p| !hw_tlb.test(p)));

                attacker.observe(entry, &page_table, &hw_tlb, &mut pte_observations);
            });
        }

        attacker.handle_step(&mut pte_observations);

        // Simulate interrupt if attacker can trigger an interrupt now
        if can_trigger_interrupt {
            attacker.handle_interrupt(&page_table, &mut pte_observations);

            // Interrupt causes hardware TLB flush
            hw_tlb.flush();

            // Resume to AEX handler
            if !no_prefetch {
                // TLBlur prefetches pages from PAM
                hw_tlb.update(pam.get_pam());
                pte_observations.update(pam.get_pam());

                // Prefetch stack pages
                let stack_ptr = unsafe { enclave_ref.gprsgx_region().fields.rsp };
                if stack_ptr >= enclave_ref.base() as u64 && stack_ptr <= enclave_ref.limit() as u64
                {
                    let stack_page = (stack_ptr - enclave_ref.base() as u64) >> 12;
                    let stack_pages = (stack_page - 1..=stack_page + 1)
                        .map(|page| PageAccess {
                            read: true,
                            execute: true,
                            write: false,
                            page: page as usize,
                        })
                        .collect::<Vec<_>>();
                    hw_tlb.update(stack_pages.iter());
                    pte_observations.update(stack_pages.iter());
                }

                // Prefetch the PAM update code
                let tlblur_tlb_update_page =
                    (pam_update_code_address - enclave_ref.base() as u64) >> 12;
                let page_access = PageAccess {
                    read: true,
                    execute: true,
                    write: false,
                    page: tlblur_tlb_update_page as usize,
                };
                hw_tlb.update(std::iter::once(&page_access));
                pte_observations.update(std::iter::once(&page_access));

                let counter_page = (pam_counter_address as u64 - enclave_ref.base() as u64) >> 12;
                let page_access = PageAccess {
                    read: true,
                    execute: false,
                    write: true,
                    page: counter_page as usize,
                };
                hw_tlb.update(std::iter::once(&page_access));
                pte_observations.update(std::iter::once(&page_access));

                let pam_page = (pam_address - enclave_ref.base() as u64) >> 12;
                let pam_pages = (pam_page
                    ..=pam_page + (pam.pam_buffer.len() as u64 * 8) / PAGE_SIZE_4KiB as u64)
                    .map(|page| PageAccess {
                        read: true,
                        execute: false,
                        write: true,
                        page: page as usize,
                    })
                    .collect::<Vec<_>>();
                hw_tlb.update(pam_pages.iter());
                pte_observations.update(pam_pages.iter());
            }
        } else {
            // We triggered a trap interrupt, but the attacker would not have interrupted...
            // Now the real hardware TLB is flushed, nothing we can do about that now.
            //
            // Instead we simulate the hardware TLB.

            // If the attacker doesn't interrupt, the hardware TLB would not be flushed,
            // so we update it to take the accesses of the current instruction into account.
            hw_tlb.update(page_table.get_all_accessed_pages());
        }

        // Clear all A/D bits so we can accurately record page accesses
        page_table.clear_all_ad_bits();
    })?;

    let lib = ProfilerLibrary::new(&library)?;
    run_profiler(lib, &enclave, &args.args);

    Ok(())
}
