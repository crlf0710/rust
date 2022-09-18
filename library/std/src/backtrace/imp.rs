use core::ffi::c_void;
use core::fmt;
use core::str;
use rustc_demangle::{try_demangle, Demangle};

const HEX_WIDTH: usize = 2 + 2 * core::mem::size_of::<usize>();

#[derive(Debug)]
pub enum BytesOrWideString<'a> {
    /// A slice, typically provided on Unix platforms.
    Bytes(&'a [u8]),
    /// Wide strings typically from Windows.
    Wide(&'a [u16]),
}

#[allow(dead_code)]
pub struct SymbolName<'a> {
    bytes: &'a [u8],
    demangled: Option<Demangle<'a>>,
}

impl<'a> SymbolName<'a> {
    /// Creates a new symbol name from the raw underlying bytes.
    pub fn new(bytes: &'a [u8]) -> SymbolName<'a> {
        let str_bytes = str::from_utf8(bytes).ok();
        let demangled = str_bytes.and_then(|s| try_demangle(s).ok());

        SymbolName { bytes: bytes, demangled: demangled }
    }

    /// Returns the raw (mangled) symbol name as a `str` if the symbol is valid utf-8.
    ///
    /// Use the `Display` implementation if you want the demangled version.
    pub fn as_str(&self) -> Option<&'a str> {
        self.demangled.as_ref().map(|s| s.as_str()).or_else(|| str::from_utf8(self.bytes).ok())
    }

    /// Returns the raw symbol name as a list of bytes
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

impl<'a> fmt::Display for SymbolName<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref s) = self.demangled {
            s.fmt(f)
        } else {
            format_symbol_name(fmt::Display::fmt, self.bytes, f)
        }
    }
}

fn format_symbol_name(
    fmt: fn(&str, &mut fmt::Formatter<'_>) -> fmt::Result,
    mut bytes: &[u8],
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    while bytes.len() > 0 {
        match str::from_utf8(bytes) {
            Ok(name) => {
                fmt(name, f)?;
                break;
            }
            Err(err) => {
                fmt("\u{FFFD}", f)?;

                match err.error_len() {
                    Some(len) => bytes = &bytes[err.valid_up_to() + len..],
                    None => break,
                }
            }
        }
    }
    Ok(())
}

pub struct BacktraceFmt<'a, 'b> {
    fmt: &'a mut fmt::Formatter<'b>,
    frame_index: usize,
    format: PrintFmt,
    print_path:
        &'a mut (dyn FnMut(&mut fmt::Formatter<'_>, BytesOrWideString<'_>) -> fmt::Result + 'b),
}

impl<'a, 'b> BacktraceFmt<'a, 'b> {
    /// Create a new `BacktraceFmt` which will write output to the provided
    /// `fmt`.
    ///
    /// The `format` argument will control the style in which the backtrace is
    /// printed, and the `print_path` argument will be used to print the
    /// `BytesOrWideString` instances of filenames. This type itself doesn't do
    /// any printing of filenames, but this callback is required to do so.
    pub fn new(
        fmt: &'a mut fmt::Formatter<'b>,
        format: PrintFmt,
        print_path: &'a mut (dyn FnMut(&mut fmt::Formatter<'_>, BytesOrWideString<'_>) -> fmt::Result
                     + 'b),
    ) -> Self {
        BacktraceFmt { fmt, frame_index: 0, format, print_path }
    }

    /// Prints a preamble for the backtrace about to be printed.
    ///
    /// This is required on some platforms for backtraces to be fully
    /// symbolicated later, and otherwise this should just be the first method
    /// you call after creating a `BacktraceFmt`.
    pub fn add_context(&mut self) -> fmt::Result {
        #[cfg(target_os = "fuchsia")]
        fuchsia::print_dso_context(self.fmt)?;
        Ok(())
    }

    /// Adds a frame to the backtrace output.
    ///
    /// This commit returns an RAII instance of a `BacktraceFrameFmt` which can be used
    /// to actually print a frame, and on destruction it will increment the
    /// frame counter.
    pub fn frame(&mut self) -> BacktraceFrameFmt<'_, 'a, 'b> {
        BacktraceFrameFmt { fmt: self, symbol_index: 0 }
    }

    /// Completes the backtrace output.
    ///
    /// This is currently a no-op but is added for future compatibility with
    /// backtrace formats.
    pub fn finish(&mut self) -> fmt::Result {
        // Currently a no-op-- including this hook to allow for future additions.
        Ok(())
    }
}

pub struct BacktraceFrameFmt<'fmt, 'a, 'b> {
    fmt: &'fmt mut BacktraceFmt<'a, 'b>,
    symbol_index: usize,
}

impl BacktraceFrameFmt<'_, '_, '_> {
    /// Prints a `BacktraceFrame` with this frame formatter.
    ///
    /// This will recursively print all `BacktraceSymbol` instances within the
    /// `BacktraceFrame`.
    ///
    /// # Required features
    ///
    /// This function requires the `std` feature of the `backtrace` crate to be
    /// enabled, and the `std` feature is enabled by default.
    #[cfg(feature = "std")]
    pub fn backtrace_frame(&mut self, frame: &BacktraceFrame) -> fmt::Result {
        let symbols = frame.symbols();
        for symbol in symbols {
            self.backtrace_symbol(frame, symbol)?;
        }
        if symbols.is_empty() {
            self.print_raw(frame.ip(), None, None, None)?;
        }
        Ok(())
    }

    /// Prints a `BacktraceSymbol` within a `BacktraceFrame`.
    ///
    /// # Required features
    ///
    /// This function requires the `std` feature of the `backtrace` crate to be
    /// enabled, and the `std` feature is enabled by default.
    #[cfg(feature = "std")]
    pub fn backtrace_symbol(
        &mut self,
        frame: &BacktraceFrame,
        symbol: &BacktraceSymbol,
    ) -> fmt::Result {
        self.print_raw_with_column(
            frame.ip(),
            symbol.name(),
            // TODO: this isn't great that we don't end up printing anything
            // with non-utf8 filenames. Thankfully almost everything is utf8 so
            // this shouldn't be too too bad.
            symbol.filename().and_then(|p| Some(BytesOrWideString::Bytes(p.to_str()?.as_bytes()))),
            symbol.lineno(),
            symbol.colno(),
        )?;
        Ok(())
    }

    /// Prints a raw traced `Frame` and `Symbol`, typically from within the raw
    /// callbacks of this crate.
    pub fn symbol(&mut self, frame: &Frame, symbol: &super::Symbol) -> fmt::Result {
        self.print_raw_with_column(
            frame.ip(),
            symbol.name(),
            symbol.filename_raw(),
            symbol.lineno(),
            symbol.colno(),
        )?;
        Ok(())
    }

    /// Adds a raw frame to the backtrace output.
    ///
    /// This method, unlike the previous, takes the raw arguments in case
    /// they're being source from different locations. Note that this may be
    /// called multiple times for one frame.
    pub fn print_raw(
        &mut self,
        frame_ip: *mut c_void,
        symbol_name: Option<SymbolName<'_>>,
        filename: Option<BytesOrWideString<'_>>,
        lineno: Option<u32>,
    ) -> fmt::Result {
        self.print_raw_with_column(frame_ip, symbol_name, filename, lineno, None)
    }

    /// Adds a raw frame to the backtrace output, including column information.
    ///
    /// This method, like the previous, takes the raw arguments in case
    /// they're being source from different locations. Note that this may be
    /// called multiple times for one frame.
    pub fn print_raw_with_column(
        &mut self,
        frame_ip: *mut c_void,
        symbol_name: Option<SymbolName<'_>>,
        filename: Option<BytesOrWideString<'_>>,
        lineno: Option<u32>,
        colno: Option<u32>,
    ) -> fmt::Result {
        // Fuchsia is unable to symbolize within a process so it has a special
        // format which can be used to symbolize later. Print that instead of
        // printing addresses in our own format here.
        if cfg!(target_os = "fuchsia") {
            self.print_raw_fuchsia(frame_ip)?;
        } else {
            self.print_raw_generic(frame_ip, symbol_name, filename, lineno, colno)?;
        }
        self.symbol_index += 1;
        Ok(())
    }

    #[allow(unused_mut)]
    fn print_raw_generic(
        &mut self,
        mut frame_ip: *mut c_void,
        symbol_name: Option<SymbolName<'_>>,
        filename: Option<BytesOrWideString<'_>>,
        lineno: Option<u32>,
        colno: Option<u32>,
    ) -> fmt::Result {
        // No need to print "null" frames, it basically just means that the
        // system backtrace was a bit eager to trace back super far.
        if let PrintFmt::Short = self.fmt.format {
            if frame_ip.is_null() {
                return Ok(());
            }
        }

        // To reduce TCB size in Sgx enclave, we do not want to implement symbol
        // resolution functionality.  Rather, we can print the offset of the
        // address here, which could be later mapped to correct function.
        #[cfg(all(feature = "std", target_env = "sgx", target_vendor = "fortanix"))]
        {
            let image_base = std::os::fortanix_sgx::mem::image_base();
            frame_ip = usize::wrapping_sub(frame_ip as usize, image_base as _) as _;
        }

        // Print the index of the frame as well as the optional instruction
        // pointer of the frame. If we're beyond the first symbol of this frame
        // though we just print appropriate whitespace.
        if self.symbol_index == 0 {
            write!(self.fmt.fmt, "{:4}: ", self.fmt.frame_index)?;
            if let PrintFmt::Full = self.fmt.format {
                write!(self.fmt.fmt, "{:1$?} - ", frame_ip, HEX_WIDTH)?;
            }
        } else {
            write!(self.fmt.fmt, "      ")?;
            if let PrintFmt::Full = self.fmt.format {
                write!(self.fmt.fmt, "{:1$}", "", HEX_WIDTH + 3)?;
            }
        }

        // Next up write out the symbol name, using the alternate formatting for
        // more information if we're a full backtrace. Here we also handle
        // symbols which don't have a name,
        match (symbol_name, &self.fmt.format) {
            (Some(name), PrintFmt::Short) => write!(self.fmt.fmt, "{:#}", name)?,
            (Some(name), PrintFmt::Full) => write!(self.fmt.fmt, "{}", name)?,
            (None, _) | (_, PrintFmt::__Nonexhaustive) => write!(self.fmt.fmt, "<unknown>")?,
        }
        self.fmt.fmt.write_str("\n")?;

        // And last up, print out the filename/line number if they're available.
        if let (Some(file), Some(line)) = (filename, lineno) {
            self.print_fileline(file, line, colno)?;
        }

        Ok(())
    }

    fn print_fileline(
        &mut self,
        file: BytesOrWideString<'_>,
        line: u32,
        colno: Option<u32>,
    ) -> fmt::Result {
        // Filename/line are printed on lines under the symbol name, so print
        // some appropriate whitespace to sort of right-align ourselves.
        if let PrintFmt::Full = self.fmt.format {
            write!(self.fmt.fmt, "{:1$}", "", HEX_WIDTH)?;
        }
        write!(self.fmt.fmt, "             at ")?;

        // Delegate to our internal callback to print the filename and then
        // print out the line number.
        (self.fmt.print_path)(self.fmt.fmt, file)?;
        write!(self.fmt.fmt, ":{}", line)?;

        // Add column number, if available.
        if let Some(colno) = colno {
            write!(self.fmt.fmt, ":{}", colno)?;
        }

        write!(self.fmt.fmt, "\n")?;
        Ok(())
    }

    fn print_raw_fuchsia(&mut self, frame_ip: *mut c_void) -> fmt::Result {
        // We only care about the first symbol of a frame
        if self.symbol_index == 0 {
            self.fmt.fmt.write_str("{{{bt:")?;
            write!(self.fmt.fmt, "{}:{:?}", self.fmt.frame_index, frame_ip)?;
            self.fmt.fmt.write_str("}}}\n")?;
        }
        Ok(())
    }
}

impl Drop for BacktraceFrameFmt<'_, '_, '_> {
    fn drop(&mut self) {
        self.fmt.frame_index += 1;
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum PrintFmt {
    /// Prints a terser backtrace which ideally only contains relevant information
    Short,
    /// Prints a backtrace that contains all possible information
    Full,
    #[doc(hidden)]
    __Nonexhaustive,
}

pub unsafe fn trace_unsynchronized<F: FnMut(&Frame) -> bool>(mut cb: F) {
    trace_imp(&mut cb)
}

pub unsafe fn resolve_frame_unsynchronized<F>(frame: &Frame, mut cb: F)
where
    F: FnMut(&Symbol),
{
    symbolize_imp::resolve(ResolveWhat::Frame(frame), &mut cb)
}

/// A trait representing the resolution of a symbol in a file.
///
/// This trait is yielded as a trait object to the closure given to the
/// `backtrace::resolve` function, and it is virtually dispatched as it's
/// unknown which implementation is behind it.
///
/// A symbol can give contextual information about a function, for example the
/// name, filename, line number, precise address, etc. Not all information is
/// always available in a symbol, however, so all methods return an `Option`.
pub struct Symbol {
    // TODO: this lifetime bound needs to be persisted eventually to `Symbol`,
    // but that's currently a breaking change. For now this is safe since
    // `Symbol` is only ever handed out by reference and can't be cloned.
    inner: symbolize_imp::Symbol<'static>,
}

impl Symbol {
    /// Returns the name of this function.
    ///
    /// The returned structure can be used to query various properties about the
    /// symbol name:
    ///
    /// * The `Display` implementation will print out the demangled symbol.
    /// * The raw `str` value of the symbol can be accessed (if it's valid
    ///   utf-8).
    /// * The raw bytes for the symbol name can be accessed.
    pub fn name(&self) -> Option<SymbolName<'_>> {
        self.inner.name()
    }

    /// Returns the starting address of this function.
    pub fn addr(&self) -> Option<*mut c_void> {
        self.inner.addr().map(|p| p as *mut _)
    }

    /// Returns the raw filename as a slice. This is mainly useful for `no_std`
    /// environments.
    pub fn filename_raw(&self) -> Option<BytesOrWideString<'_>> {
        self.inner.filename_raw()
    }

    /// Returns the column number for where this symbol is currently executing.
    ///
    /// Only gimli currently provides a value here and even then only if `filename`
    /// returns `Some`, and so it is then consequently subject to similar caveats.
    pub fn colno(&self) -> Option<u32> {
        self.inner.colno()
    }

    /// Returns the line number for where this symbol is currently executing.
    ///
    /// This return value is typically `Some` if `filename` returns `Some`, and
    /// is consequently subject to similar caveats.
    pub fn lineno(&self) -> Option<u32> {
        self.inner.lineno()
    }

    /// Returns the file name where this function was defined.
    ///
    /// This is currently only available when libbacktrace or gimli is being
    /// used (e.g. unix platforms other) and when a binary is compiled with
    /// debuginfo. If neither of these conditions is met then this will likely
    /// return `None`.
    ///
    /// # Required features
    ///
    /// This function requires the `std` feature of the `backtrace` crate to be
    /// enabled, and the `std` feature is enabled by default.
    #[cfg(feature = "std")]
    #[allow(unreachable_code)]
    pub fn filename(&self) -> Option<&Path> {
        self.inner.filename()
    }
}

pub enum ResolveWhat<'a> {
    Address(*mut c_void),
    Frame(&'a Frame),
}

// IP values from stack frames are typically (always?) the instruction
// *after* the call that's the actual stack trace. Symbolizing this on
// causes the filename/line number to be one ahead and perhaps into
// the void if it's near the end of the function.
//
// This appears to basically always be the case on all platforms, so we always
// subtract one from a resolved ip to resolve it to the previous call
// instruction instead of the instruction being returned to.
//
// Ideally we would not do this. Ideally we would require callers of the
// `resolve` APIs here to manually do the -1 and account that they want location
// information for the *previous* instruction, not the current. Ideally we'd
// also expose on `Frame` if we are indeed the address of the next instruction
// or the current.
//
// For now though this is a pretty niche concern so we just internally always
// subtract one. Consumers should keep working and getting pretty good results,
// so we should be good enough.
fn adjust_ip(a: *mut c_void) -> *mut c_void {
    if a.is_null() {
        a
    } else {
        (a as usize - 1) as *mut c_void
    }
}

impl<'a> ResolveWhat<'a> {
    #[allow(dead_code)]
    fn address_or_ip(&self) -> *mut c_void {
        match self {
            ResolveWhat::Address(a) => adjust_ip(*a),
            ResolveWhat::Frame(f) => adjust_ip(f.ip()),
        }
    }
}

#[derive(Clone)]
pub struct Frame {
    pub(crate) inner: FrameImp,
}

impl Frame {
    /// Returns the current instruction pointer of this frame.
    ///
    /// This is normally the next instruction to execute in the frame, but not
    /// all implementations list this with 100% accuracy (but it's generally
    /// pretty close).
    ///
    /// It is recommended to pass this value to `backtrace::resolve` to turn it
    /// into a symbol name.
    pub fn ip(&self) -> *mut c_void {
        self.inner.ip()
    }

    /// Returns the current stack pointer of this frame.
    ///
    /// In the case that a backend cannot recover the stack pointer for this
    /// frame, a null pointer is returned.
    pub fn sp(&self) -> *mut c_void {
        self.inner.sp()
    }

    /// Returns the starting symbol address of the frame of this function.
    ///
    /// This will attempt to rewind the instruction pointer returned by `ip` to
    /// the start of the function, returning that value. In some cases, however,
    /// backends will just return `ip` from this function.
    ///
    /// The returned value can sometimes be used if `backtrace::resolve` failed
    /// on the `ip` given above.
    pub fn symbol_address(&self) -> *mut c_void {
        self.inner.symbol_address()
    }

    /// Returns the base address of the module to which the frame belongs.
    pub fn module_base_address(&self) -> Option<*mut c_void> {
        self.inner.module_base_address()
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("ip", &self.ip())
            .field("symbol_address", &self.symbol_address())
            .finish()
    }
}

cfg_if::cfg_if! {
    // This needs to come first, to ensure that
    // Miri takes priority over the host platform
    if #[cfg(miri)] {
        pub(crate) mod backtrace_miri;
        use self::backtrace_miri::trace as trace_imp;
        pub(crate) use self::backtrace_miri::Frame as FrameImp;
    } else if #[cfg(
        any(
            all(
                unix,
                not(target_os = "emscripten"),
                not(all(target_os = "ios", target_arch = "arm")),
            ),
            all(
                target_env = "sgx",
                target_vendor = "fortanix",
            ),
        )
    )] {
        mod backtrace_libunwind;
        use self::backtrace_libunwind::trace as trace_imp;
        pub(crate) use self::backtrace_libunwind::Frame as FrameImp;
    } else if #[cfg(all(windows, not(target_vendor = "uwp")))] {
        mod backtrace_dbghelp;
        use self::backtrace_dbghelp::trace as trace_imp;
        pub(crate) use self::backtrace_dbghelp::Frame as FrameImp;
        #[cfg(target_env = "msvc")] // only used in dbghelp symbolize
        pub(crate) use self::backtrace_dbghelp::StackFrame;
    } else {
        mod noop;
        use self::noop::trace as trace_imp;
        pub(crate) use self::noop::Frame as FrameImp;
    }
}

cfg_if::cfg_if! {
    if #[cfg(miri)] {
        mod symbolize_miri;
        use symbolize_miri as symbolize_imp;
    } else if #[cfg(all(windows, target_env = "msvc", not(target_vendor = "uwp")))] {
        mod symbolize_dbghelp;
        use symbolize_dbghelp as symbolize_imp;
    } else if #[cfg(all(
        any(unix, windows),
        not(target_vendor = "uwp"),
        not(target_os = "emscripten"),
        any(not(backtrace_in_libstd), feature = "backtrace"),
    ))] {
        mod symbolize_gimli;
        use symbolize_gimli as symbolize_imp;
    } else {
        mod symbolize_noop;
        use symbolize_noop as symbolize_imp;
    }
}
