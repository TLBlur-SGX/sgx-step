use std::error::Error;

use clap::Parser;
use sgx_profiler::{
    create_dumper, create_enclave, create_trap_handler,
    dump::{RSet, VCDDumper},
    run_profiler, PageTable, ProfilerLibrary,
};

/// SGX page access profiler
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

    /// Arguments to pass to the profiler_run function
    #[arg(long, value_parser, num_args = 1.., value_delimiter = ' ')]
    args: Vec<String>,

    /// Write erip to VCD output
    #[arg(long = "erip")]
    write_erip: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let enclave = create_enclave(&args.enclave)?;

    let mut dumper: VCDDumper<RSet> = create_dumper(&enclave, &args.trace_output);
    let mut page_table = PageTable::new(&enclave);
    let write_erip = args.write_erip;

    // let (signal_handle, handler_thread) = create_trap_handler(move || {
    create_trap_handler(move || {
        // Write to VCD trace
        dumper.next_step(|entry| {
            if write_erip {
                entry.write_erip();
            }

            // Check which pages were accessed and write to VCD
            page_table.update_page_accesses();
            entry.write_page_accesses(page_table.get_all_accessed_pages());
        });

        // Clear all A/D bits in enclave page table
        page_table.clear_all_ad_bits();
    })?;

    let library = unsafe { libloading::Library::new(&args.so)? };
    let lib = ProfilerLibrary::new(&library)?;
    run_profiler(lib, &enclave, &args.args);

    Ok(())
}
