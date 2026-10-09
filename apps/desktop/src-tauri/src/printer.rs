pub struct PrinterProbe {
    pub state: &'static str,
    pub detail: String,
}

#[cfg(target_os = "windows")]
mod platform {
    use super::PrinterProbe;
    use std::{
        ffi::c_void,
        fs::OpenOptions,
        io::Write,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Graphics::Printing::{
            ClosePrinter, EndDocPrinter, EndPagePrinter, GetDefaultPrinterW, OpenPrinterW,
            StartDocPrinterW, StartPagePrinter, WritePrinter, DOC_INFO_1W,
        },
    };

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn default_printer_name() -> Result<Vec<u16>, String> {
        let mut length = 0u32;
        unsafe { GetDefaultPrinterW(null_mut(), &mut length) };
        if length == 0 {
            return Err("Windows has no default receipt printer".into());
        }
        let mut buffer = vec![0u16; length as usize];
        if unsafe { GetDefaultPrinterW(buffer.as_mut_ptr(), &mut length) } == 0 {
            return Err(format!(
                "GetDefaultPrinterW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(buffer)
    }

    fn print_raw(target: Option<&str>, document_name: &str, bytes: &[u8]) -> Result<(), String> {
        let printer = match target {
            Some(name) if !name.trim().is_empty() => wide(name.trim()),
            _ => default_printer_name()?,
        };
        let mut handle: HANDLE = null_mut();
        if unsafe { OpenPrinterW(printer.as_ptr(), &mut handle, null()) } == 0 {
            return Err(format!(
                "OpenPrinterW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let document = wide(document_name);
        let datatype = wide("RAW");
        let info = DOC_INFO_1W {
            pDocName: document.as_ptr() as *mut _,
            pOutputFile: null_mut(),
            pDatatype: datatype.as_ptr() as *mut _,
        };
        let result = (|| {
            if unsafe { StartDocPrinterW(handle, 1, &info as *const _ as *const u8) } == 0 {
                return Err(format!(
                    "StartDocPrinterW failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if unsafe { StartPagePrinter(handle) } == 0 {
                unsafe { EndDocPrinter(handle) };
                return Err(format!(
                    "StartPagePrinter failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let mut written = 0u32;
            let ok = unsafe {
                WritePrinter(
                    handle,
                    bytes.as_ptr() as *const c_void,
                    bytes.len() as u32,
                    &mut written,
                )
            };
            unsafe {
                EndPagePrinter(handle);
                EndDocPrinter(handle)
            };
            if ok == 0 {
                return Err(format!(
                    "WritePrinter failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if written as usize != bytes.len() {
                return Err(format!(
                    "Windows spooler accepted only {written} of {} bytes",
                    bytes.len()
                ));
            }
            Ok(())
        })();
        unsafe { ClosePrinter(handle) };
        result
    }

    pub fn probe(transport: &str, target: Option<&str>) -> PrinterProbe {
        match transport {
            "WINDOWS_SPOOLER" => {
                let printer = match target {
                    Some(name) if !name.trim().is_empty() => Ok(wide(name.trim())),
                    _ => default_printer_name(),
                };
                let printer = match printer {
                    Ok(value) => value,
                    Err(error) => {
                        return PrinterProbe {
                            state: "UNAVAILABLE",
                            detail: error,
                        }
                    }
                };
                let mut handle: HANDLE = null_mut();
                if unsafe { OpenPrinterW(printer.as_ptr(), &mut handle, null()) } == 0 {
                    return PrinterProbe {
                        state: "UNAVAILABLE",
                        detail: format!(
                            "Windows spooler printer cannot be opened: {}",
                            std::io::Error::last_os_error()
                        ),
                    };
                }
                unsafe { ClosePrinter(handle) };
                PrinterProbe {
                    state: "REACHABLE",
                    detail: "Windows spooler printer handle opened successfully".into(),
                }
            }
            "SERIAL" => {
                let Some(port) = target.filter(|value| !value.trim().is_empty()) else {
                    return PrinterProbe {
                        state: "MISCONFIGURED",
                        detail: "Serial printer target is missing".into(),
                    };
                };
                let path = if port.starts_with(r"\\.\") {
                    port.to_string()
                } else {
                    format!(r"\\.\{}", port.trim())
                };
                match OpenOptions::new().write(true).open(path) {
                    Ok(_) => PrinterProbe {
                        state: "REACHABLE",
                        detail: "Serial printer port opened successfully".into(),
                    },
                    Err(error) => PrinterProbe {
                        state: "UNAVAILABLE",
                        detail: format!("Serial printer port cannot be opened: {error}"),
                    },
                }
            }
            _ => PrinterProbe {
                state: "MISCONFIGURED",
                detail: "Unsupported printer transport".into(),
            },
        }
    }

    pub fn print_bytes(
        transport: &str,
        target: Option<&str>,
        document_name: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        match transport {
            "WINDOWS_SPOOLER" => print_raw(target, document_name, bytes),
            "SERIAL" => {
                let port = target
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "serial printer target is required".to_string())?;
                let path = if port.starts_with(r"\\.\") {
                    port.to_string()
                } else {
                    format!(r"\\.\{}", port.trim())
                };
                let mut output = OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .map_err(|error| format!("open serial printer {port}: {error}"))?;
                output
                    .write_all(bytes)
                    .and_then(|_| output.flush())
                    .map_err(|error| format!("write serial printer {port}: {error}"))
            }
            _ => Err("unsupported printer transport".into()),
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::PrinterProbe;

    pub fn probe(_transport: &str, _target: Option<&str>) -> PrinterProbe {
        PrinterProbe {
            state: "UNSUPPORTED_PLATFORM",
            detail: "Windows printer probing is unavailable on this operating system".into(),
        }
    }

    pub fn print_bytes(
        _transport: &str,
        _target: Option<&str>,
        _document_name: &str,
        _bytes: &[u8],
    ) -> Result<(), String> {
        Err("Windows printer transports are unavailable on this operating system".into())
    }
}

pub use platform::{print_bytes, probe};

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    #[test]
    fn probe_reports_unsupported_platform_without_exposing_target() {
        let result = super::probe("SERIAL", Some("COM-SECRET"));
        assert_eq!(result.state, "UNSUPPORTED_PLATFORM");
        assert!(!result.detail.contains("COM-SECRET"));
    }
}
