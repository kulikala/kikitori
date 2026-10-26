use std::io::{Read, Write};

#[cfg(unix)]
static ORIGINAL_STDERR: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
#[cfg(windows)]
static ORIGINAL_STDERR: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

pub fn panic_error(detail: &str) {
    let record = serde_json::json!({"type":"error", "code":"internal", "message":"内部エラーが発生しました。", "hint":"コマンドとバージョンと detail を添えて、この問題を報告してください。", "detail": detail}).to_string() + "\n";
    #[cfg(unix)]
    unsafe {
        let fd = ORIGINAL_STDERR.load(std::sync::atomic::Ordering::Relaxed);
        if fd >= 0 {
            libc::write(fd, record.as_ptr().cast(), record.len());
            return;
        }
    }
    #[cfg(windows)]
    unsafe {
        let fd = ORIGINAL_STDERR.load(std::sync::atomic::Ordering::Relaxed);
        if fd >= 0 {
            libc::write(fd, record.as_ptr().cast(), record.len() as u32);
            return;
        }
    }
    crate::log::error(
        "internal",
        &format!("内部エラーが発生しました（{detail}）。"),
        "コマンドとバージョンを添えて、この問題を報告してください。",
    );
}

/// Write one line of kikitori's own output to the real stderr, past any capture in progress.
pub fn write_line(line: &str) {
    #[cfg(any(unix, windows))]
    {
        let fd = ORIGINAL_STDERR.load(std::sync::atomic::Ordering::Relaxed);
        if fd >= 0 {
            let record = format!("{line}\n");
            let mut rest = record.as_bytes();
            while !rest.is_empty() {
                let written = unsafe { libc::write(fd, rest.as_ptr().cast(), rest.len() as _) };
                if written <= 0 {
                    return;
                }
                rest = &rest[written as usize..];
            }
            return;
        }
    }
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// Run a native-library call with fd 2 redirected, then re-emit each captured line.
pub fn capture_stderr<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(unix)]
    let (result, lines) = unsafe {
        use std::os::fd::FromRawFd;
        let mut pipe = [0; 2];
        if libc::pipe(pipe.as_mut_ptr()) != 0 {
            return f();
        }
        let saved = libc::dup(2);
        if saved < 0 || libc::dup2(pipe[1], 2) < 0 {
            libc::close(pipe[0]);
            libc::close(pipe[1]);
            return f();
        }
        ORIGINAL_STDERR.store(saved, std::sync::atomic::Ordering::Relaxed);
        libc::close(pipe[1]);
        // Children get fd 2 (the write end) as their stderr; they need neither of these.
        libc::fcntl(pipe[0], libc::F_SETFD, libc::FD_CLOEXEC);
        libc::fcntl(saved, libc::F_SETFD, libc::FD_CLOEXEC);
        let mut reader = std::fs::File::from_raw_fd(pipe[0]);
        let thread = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = reader.read_to_end(&mut bytes);
            bytes
        });
        let result = f();
        let _ = std::io::stderr().flush();
        libc::dup2(saved, 2);
        libc::close(saved);
        ORIGINAL_STDERR.store(-1, std::sync::atomic::Ordering::Relaxed);
        (result, thread.join().unwrap_or_default())
    };
    #[cfg(windows)]
    let (result, lines) = unsafe {
        use std::os::windows::io::FromRawHandle;
        let mut pipe = [0; 2];
        // Not inheritable: a child process (the uninstall helper, `ollama serve`) holding the write end
        // would keep the reader below from seeing EOF until that child exits.
        if libc::pipe(pipe.as_mut_ptr(), 8192, libc::O_BINARY | libc::O_NOINHERIT) != 0 {
            return f();
        }
        let saved = libc::dup(2);
        if saved < 0 || libc::dup2(pipe[1], 2) != 0 {
            libc::close(pipe[0]);
            libc::close(pipe[1]);
            return f();
        }
        ORIGINAL_STDERR.store(saved, std::sync::atomic::Ordering::Relaxed);
        // SetStdHandle takes an OS handle; fd 2 now refers to the pipe's write end. _dup2 makes that
        // handle inheritable, so clear the flag for the same reason as above.
        let write_end = libc::get_osfhandle(2) as windows_sys::Win32::Foundation::HANDLE;
        let _ = windows_sys::Win32::Foundation::SetHandleInformation(
            write_end,
            windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT,
            0,
        );
        let _ = windows_sys::Win32::System::Console::SetStdHandle(
            windows_sys::Win32::System::Console::STD_ERROR_HANDLE,
            write_end,
        );
        libc::close(pipe[1]);
        let handle = libc::get_osfhandle(pipe[0]) as *mut _;
        let mut reader = std::fs::File::from_raw_handle(handle);
        let thread = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = reader.read_to_end(&mut bytes);
            bytes
        });
        let result = f();
        let _ = std::io::stderr().flush();
        libc::dup2(saved, 2);
        libc::close(saved);
        ORIGINAL_STDERR.store(-1, std::sync::atomic::Ordering::Relaxed);
        let _ = windows_sys::Win32::System::Console::SetStdHandle(
            windows_sys::Win32::System::Console::STD_ERROR_HANDLE,
            libc::get_osfhandle(2) as _,
        );
        (result, thread.join().unwrap_or_default())
    };
    #[cfg(not(any(unix, windows)))]
    let (result, lines) = (f(), Vec::new());
    for line in String::from_utf8_lossy(&lines).lines() {
        crate::log::captured(line);
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn captures_c_level_stderr_write() {
        crate::log::configure(true, false, false);
        let value = super::capture_stderr(|| unsafe {
            let bytes = b"native warning from C\n";
            libc::write(2, bytes.as_ptr().cast(), bytes.len() as _);
            17
        });
        assert_eq!(value, 17);
    }
}
