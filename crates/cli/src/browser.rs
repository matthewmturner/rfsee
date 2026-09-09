use std::{
    io,
    process::{Command, Stdio},
};

pub(crate) fn open(url: &str) -> io::Result<()> {
    let mut command = browser_command(url);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn browser_command(url: &str) -> Command {
    // macOS `open` opens a URL with its registered default application.
    // https://developer.apple.com/library/archive/documentation/OpenSource/Conceptual/ShellScripting/CommandLInePrimer/CommandLine.html
    let mut command = Command::new("open");
    command.arg(url);
    command
}

#[cfg(target_os = "linux")]
fn browser_command(url: &str) -> Command {
    // xdg-open opens a URL in the user's preferred application.
    // https://portland.freedesktop.org/doc/xdg-open.html
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
}

#[cfg(target_os = "windows")]
fn browser_command(url: &str) -> Command {
    // `start` launches a URL through its Windows file association. The empty argument is the
    // window title consumed by `start` before the URL.
    // https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/start
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn browser_command(_url: &str) -> Command {
    Command::new("rfsee-browser-launch-is-not-supported-on-this-platform")
}

#[cfg(test)]
mod tests {
    use super::browser_command;
    use std::ffi::OsStr;

    #[test]
    fn passes_url_as_one_argument() {
        let url = "https://www.rfc-editor.org/rfc/rfc9110";
        let command = browser_command(url);
        assert_eq!(command.get_args().last(), Some(OsStr::new(url)));
    }
}
