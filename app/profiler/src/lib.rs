pub mod dump;

use dump::{TracePageSet, VCDDumper};
use libloading::Symbol;
use nix::libc::{self, mlock};
use nix::sys::signal;
use sgx_step::{page_table::PageTableEntry, sgx_step_sys::PAGE_SIZE_4KiB, Enclave, EnclaveRef};

use once_cell::sync::OnceCell;
use std::sync::Mutex;
use std::{
    error::Error,
    ffi::{c_char, c_void, CString},
    path::Path,
};

pub use sgx_step;
pub use sgx_urts_sys;

/// Represents an access to a page with certain permissions
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PageAccess {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub page: usize,
}

impl PageAccess {
    pub fn covers(&self, other: &Self) -> bool {
        if self.page == other.page {
            let mut result = true;
            if other.read && !self.read {
                result = false;
            }
            if other.write && !self.write {
                result = false;
            }
            if other.execute && !self.execute {
                result = false;
            }
            result
        } else {
            false
        }
    }

    pub fn union(&self, other: &Self) -> Self {
        Self {
            read: self.read || other.read,
            write: self.write || other.write,
            execute: self.execute || other.execute,
            page: self.page,
        }
    }
}

/// Interface to access and manipulate page table entries of the enclave
#[derive(Debug)]
pub struct PageTable {
    pub page_table_map: Vec<Option<PageTableEntry>>,
    pub pages: Vec<PageAccess>,
    pub accessed_ptes: Vec<(PageAccess, usize)>,
}

unsafe impl Sync for PageTable {}
unsafe impl Send for PageTable {}

impl PageTable {
    pub fn new(enclave: &EnclaveRef) -> Self {
        let mut page_table = Self {
            page_table_map: Vec::new(),
            pages: Vec::new(),
            accessed_ptes: Vec::new(),
        };

        page_table.map_all_ptes(enclave.base() as usize, enclave.end() as usize);
        page_table.clear_all_ad_bits();

        page_table
    }

    fn map_all_ptes(&mut self, base_adrs: usize, end_adrs: usize) {
        unsafe { mlock(base_adrs as *mut c_void, end_adrs - base_adrs) };
        self.page_table_map = (0..=end_adrs - base_adrs)
            .step_by(PAGE_SIZE_4KiB as usize)
            .map(|a| PageTableEntry::new(base_adrs + a))
            .collect();
    }

    pub fn clear_all_ad_bits(&mut self) {
        self.page_table_map.iter_mut().for_each(|pte| {
            if let Some(pte) = pte {
                pte.mark_not_accessed();
                pte.mark_clean();
            }
        });
    }

    pub fn get_all_accessed_pages(&self) -> impl Iterator<Item = &PageAccess> {
        self.pages.iter()
    }

    pub fn get_accessed_pages(
        &self,
        filter: impl Fn(&PageAccess) -> bool,
    ) -> impl Iterator<Item = &PageAccess> {
        self.pages.iter().filter(move |&p| filter(p))
    }

    pub fn update_page_accesses(&mut self) {
        self.pages.clear();

        for (i, pte) in self.page_table_map.iter().enumerate() {
            if let Some(pte) = pte {
                if pte.accessed() && pte.present() {
                    let p = PageAccess {
                        read: true,
                        write: pte.dirty(),
                        execute: false,
                        page: i,
                    };
                    self.pages.push(p);
                    self.accessed_ptes.push((p, i));
                }
            }
        }
    }
}

pub fn create_dumper<S: TracePageSet>(
    enclave: &EnclaveRef,
    vcd_file: impl AsRef<Path>,
) -> VCDDumper<S> {
    VCDDumper::new(
        vcd_file,
        (enclave.size() as usize) / PAGE_SIZE_4KiB as usize + 100,
    )
}

static TRAP_HANDLER: OnceCell<Mutex<Box<dyn FnMut() + Send + Sync + 'static>>> = OnceCell::new();

extern "C" fn trap_handler_wrapper(
    _signum: libc::c_int,
    _si: *mut libc::siginfo_t,
    _vuctx: *mut libc::c_void,
) {
    (TRAP_HANDLER.get().unwrap().lock().unwrap())()
}

pub fn create_trap_handler(
    handler: impl FnMut() + Send + Sync + 'static,
) -> Result<(), Box<dyn Error>> {
    TRAP_HANDLER
        .set(Mutex::new(Box::new(handler)))
        .map_err(|_| "handler already registered!")?;
    unsafe {
        signal::sigaction(
            signal::SIGTRAP,
            &signal::SigAction::new(
                signal::SigHandler::SigAction(trap_handler_wrapper),
                signal::SaFlags::SA_RESTART | signal::SaFlags::SA_SIGINFO,
                signal::SigSet::empty(),
            ),
        )
    }?;
    Ok(())
}

#[derive(Debug)]
pub struct ProfilerLibrary<'l> {
    profiler_setup: Symbol<'l, extern "C" fn(u64, u64, u64, u64, *const *const c_char)>,
    profiler_run: Symbol<'l, extern "C" fn(u64)>,
    profiler_destroy: Symbol<'l, extern "C" fn(u64)>,
}

impl<'l> ProfilerLibrary<'l> {
    pub fn new(lib: &'l libloading::Library) -> Result<Self, Box<dyn Error>> {
        unsafe {
            Ok(Self {
                profiler_setup: lib.get(b"profiler_setup")?,
                profiler_run: lib.get(b"profiler_run")?,
                profiler_destroy: lib.get(b"profiler_destroy")?,
            })
        }
    }
}

pub fn run_profiler(lib: ProfilerLibrary<'_>, enclave: &EnclaveRef, args: &[impl AsRef<str>]) {
    let ebase_address = enclave.base() as u64;
    let esize = enclave.size() as u64;

    let profiler_args = args
        .iter()
        .map(|a| CString::new(a.as_ref()).unwrap())
        .collect::<Vec<_>>();
    let profiler_args = profiler_args
        .iter()
        .map(|a| a.as_ptr())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    (*lib.profiler_setup)(
        enclave.id().sgx_eid().unwrap(),
        esize,
        ebase_address,
        profiler_args.len() as u64,
        profiler_args.as_ptr(),
    );
    (*lib.profiler_run)(enclave.id().sgx_eid().unwrap());
    (*lib.profiler_destroy)(enclave.id().sgx_eid().unwrap());
}

pub fn create_enclave(enclave: &str) -> Result<Enclave, Box<dyn Error>> {
    Enclave::new_sgx(enclave, true)
}
