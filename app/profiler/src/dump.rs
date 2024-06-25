use std::{fs::File, path::Path};

use sgx_step::sgx_step_sys::edbgrd_erip;

use crate::PageAccess;

pub trait TracePageSet: Sized {
    fn new(size: usize) -> Self;
    fn add_wires(&mut self, writer: &mut vcd::Writer<File>);
    fn init_wires(&mut self, writer: &mut vcd::Writer<File>);
    fn update_state<'a>(
        &mut self,
        writer: &mut vcd::Writer<File>,
        items: impl Iterator<Item = &'a PageAccess>,
    );
}

pub struct RWXSet {
    r: VCDStatefulSet,
    w: VCDStatefulSet,
    x: VCDStatefulSet,
    read: Vec<usize>,
    write: Vec<usize>,
    execute: Vec<usize>,
}

impl TracePageSet for RWXSet {
    fn new(size: usize) -> Self {
        Self {
            r: VCDStatefulSet::new(size, Some("r".into())),
            w: VCDStatefulSet::new(size, Some("w".into())),
            x: VCDStatefulSet::new(size, Some("x".into())),
            read: Vec::with_capacity(10),
            write: Vec::with_capacity(10),
            execute: Vec::with_capacity(10),
        }
    }

    fn add_wires(&mut self, writer: &mut vcd::Writer<File>) {
        self.r.add_wires(writer);
        self.w.add_wires(writer);
        self.x.add_wires(writer);
    }

    fn init_wires(&mut self, writer: &mut vcd::Writer<File>) {
        self.r.init_wires(writer);
        self.w.init_wires(writer);
        self.x.init_wires(writer);
    }

    fn update_state<'a>(
        &mut self,
        writer: &mut vcd::Writer<File>,
        items: impl Iterator<Item = &'a PageAccess>,
    ) {
        self.read.clear();
        self.write.clear();
        self.execute.clear();
        for item in items {
            if item.read {
                self.read.push(item.page);
            }
            if item.write {
                self.write.push(item.page);
            }
            if item.execute {
                self.execute.push(item.page);
            }
        }
        self.r.update_state(writer, &self.read);
        self.w.update_state(writer, &self.write);
        self.x.update_state(writer, &self.execute);
    }
}

pub struct RSet {
    r: VCDStatefulSet,
    read: Vec<usize>,
}

impl TracePageSet for RSet {
    fn new(size: usize) -> Self {
        Self {
            r: VCDStatefulSet::new(size, None),
            read: Vec::with_capacity(10),
        }
    }
    fn add_wires(&mut self, writer: &mut vcd::Writer<File>) {
        self.r.add_wires(writer);
    }

    fn init_wires(&mut self, writer: &mut vcd::Writer<File>) {
        self.r.init_wires(writer);
    }

    fn update_state<'a>(
        &mut self,
        writer: &mut vcd::Writer<File>,
        items: impl Iterator<Item = &'a PageAccess>,
    ) {
        self.read.clear();
        for item in items {
            if item.read {
                self.read.push(item.page);
            }
        }
        self.r.update_state(writer, &self.read);
    }
}

struct VCDStatefulSet {
    vars: Vec<vcd::IdCode>,
    state: Vec<bool>,
    wire_suffix: Option<String>,
}

impl VCDStatefulSet {
    fn new(size: usize, wire_suffix: Option<String>) -> Self {
        Self {
            state: vec![false; size],
            vars: Vec::new(),
            wire_suffix,
        }
    }

    fn add_wires(&mut self, writer: &mut vcd::Writer<File>) {
        self.vars = (0..self.state.len())
            .map(|i| {
                writer.add_wire(
                    1,
                    &self
                        .wire_suffix
                        .as_ref()
                        .map(|s| format!("_{i}_{s}"))
                        .unwrap_or(format!("_{i}")),
                )
            })
            .collect::<Result<_, _>>()
            .unwrap();
    }

    fn init_wires(&mut self, writer: &mut vcd::Writer<File>) {
        self.vars.iter().for_each(|id| {
            writer.change_scalar(id.clone(), false).unwrap();
        });
    }

    fn update_state(&mut self, writer: &mut vcd::Writer<File>, items: &[usize]) {
        for &item in items {
            if !self.state[item] {
                self.state[item] = true;
                writer.change_scalar(self.vars[item], true).unwrap();
            }
        }

        for (item, accessed) in self
            .state
            .iter_mut()
            .enumerate()
            .filter(|(_, accessed)| **accessed)
        {
            if !items.contains(&item) {
                *accessed = false;
                writer.change_scalar(self.vars[item], false).unwrap();
            }
        }
    }
}

/// `VCDDumper` is used to write profiler output to a VCD file.
///
/// The `vcd_entry` function can be called to get a handle to update
/// the VCD state at the current step during enclave execution.
/// The timestamp is incremented when this handle is dropped.
pub struct VCDDumper<S> {
    pages: S,
    rip: Option<vcd::IdCode>,
    ts: u64,
    vcd_writer: vcd::Writer<File>,
}

impl<S: TracePageSet> VCDDumper<S> {
    pub fn new(file: impl AsRef<Path>, num_pages: usize) -> Self {
        let mut vcd_writer = vcd::Writer::new(File::create(file).unwrap());
        let mut pages = S::new(num_pages);
        vcd_writer.timescale(1, vcd::TimescaleUnit::MS).unwrap();

        vcd_writer.add_module("trace").unwrap();
        pages.add_wires(&mut vcd_writer);
        let rip = Some(vcd_writer.add_wire(64, "erip").unwrap());
        vcd_writer.upscope().unwrap();

        vcd_writer.enddefinitions().unwrap();

        pages.init_wires(&mut vcd_writer);

        Self {
            pages,
            rip,
            ts: 0,
            vcd_writer,
        }
    }

    /// Write the next step of execution
    pub fn next_step<'a>(&'a mut self, f: impl FnOnce(&mut VCDEntry<'a, S>)) {
        f(&mut VCDEntry::new(self))
    }

    fn write_erip(&mut self, rip: usize) {
        self.vcd_writer
            .change_vector(
                self.rip.unwrap(),
                (0..64).rev().map(|n| (((rip >> n) & 1) != 0).into()),
            )
            .unwrap();
    }

    fn next_timestamp(&mut self) {
        self.ts += 1;
        self.vcd_writer.timestamp(self.ts).unwrap();
    }
}

/// Handle to write to a VCD file at a given step during program execution.
pub struct VCDEntry<'d, S: TracePageSet> {
    dumper: &'d mut VCDDumper<S>,
}

impl<'d, S: TracePageSet> VCDEntry<'d, S> {
    fn new(dumper: &'d mut VCDDumper<S>) -> Self {
        Self { dumper }
    }

    /// Write the erip.
    pub fn write_erip(&mut self) {
        self.dumper.write_erip(unsafe { edbgrd_erip() as usize });
    }

    /// Write the pages accessed at the current step.
    pub fn write_page_accesses<'a>(&mut self, pages: impl Iterator<Item = &'a PageAccess>) {
        self.dumper
            .pages
            .update_state(&mut self.dumper.vcd_writer, pages)
    }
}

impl<'d, S: TracePageSet> Drop for VCDEntry<'d, S> {
    fn drop(&mut self) {
        self.dumper.next_timestamp();
    }
}
