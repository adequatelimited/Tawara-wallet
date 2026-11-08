// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! What a mobile shell hands the application, and what it tells it. The
//! desktop needs none of this.
//!
//! A shell gives the app-private directory, where the store and the
//! preferences live, the platform's clipboard, its share sheet, and what
//! Android's Back does. It also gives the notices its package carries, which
//! the Licences page draws.
//! Copy goes through the shell's own write, not iced's, because iced's sets
//! none of the protection the shell's write applies. It calls
//! [`left_foreground`] when the system takes the application off the screen.
//! On iOS it also calls [`screenshot_taken`] and [`capture_changed`], which
//! the recovery phrase's screen reads. Its device fields offer the unlock
//! with the device's own check, and keep the store's password for it in the
//! system's storage. Its scan fields start a QR scan and take back the text
//! it reads, and an Android shell hands each camera frame to
//! [`preview_frame`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use iced::futures::channel::mpsc::UnboundedSender;
pub use tawara_wallet_core::SecretText;

/// What a mobile shell supplies.
#[derive(Clone, Debug)]
pub struct Host {
    /// The app-private directory: Android's `no_backup/`, iOS's
    /// Application Support. The store
    /// goes in its own folder inside it, and the preferences beside that.
    pub private_dir: PathBuf,
    /// Puts text on the platform's clipboard.
    pub copy: fn(&str),
    /// Offers `text` to the system's share sheet, with `name` as the file's
    /// name or the subject. A phone has no Downloads folder for Save artifact
    /// to write to, and the sheet is how it hands text to Files, a note or a
    /// message. iOS presents `UIActivityViewController`; Android sends
    /// `ACTION_SEND` as `text/plain` through `Intent.createChooser`. `Ok` says
    /// only that the platform was asked to show the sheet: the call returns
    /// before the sheet is presented, so a later refusal is not reported, and
    /// the application cannot learn what was done with it, so the bytes still
    /// count as not saved. `Err` says why the platform was not asked.
    pub share: fn(name: &str, text: &str) -> Result<(), String>,
    /// Android's Back: the application goes to the background, as Android
    /// 12 and later do themselves for a launcher's first activity. It is
    /// never finished. When Android destroys the Activity anyway, the
    /// process still ends after Destroy, because a restart in the same
    /// process would need the Activity's handles, the host statics and
    /// FLAG_SECURE rebuilt.
    pub back: fn(),
    /// The notices of the software the shell's package includes, word for word
    /// as its `THIRD-PARTY-NOTICES.txt` gives them. The Licences page draws
    /// them beside the wallet's licence. A phone is built from other crates
    /// than the desktop,
    /// so a phone's shell gives its own file. The desktop has no `Host`, and
    /// its page draws the wallet's file. The field is required, so a shell
    /// that leaves it out fails to build, rather than drawing the wrong file.
    pub notices: &'static str,
    /// Whether the screen is being recorded or mirrored, as the system reports
    /// it (`UIScreen.isCaptured` on iOS), read once when the application
    /// starts. Later changes come through [`capture_changed`]. `None` where a
    /// shell supplies none, as Android does, and keeps FLAG_SECURE.
    pub captured: Option<fn() -> bool>,
    /// What the device offers for unlocking the store with its own check, and
    /// what that check is called. `None`
    /// where a shell offers no device unlock, as Android does, and where the
    /// device can offer no check at all, such as one with no passcode set. The
    /// four device fields are used together: the application offers the unlock
    /// only when all four are given and the check is answered (see [`Device`]).
    pub device_check: Option<fn() -> Option<DeviceCheck>>,
    /// Keeps `password` in the system's secure storage, labelled with
    /// `identifier`, the store's 32 bytes (`store_identifier::LEN`). The shell
    /// reads `password` for this call only and keeps no
    /// copy of it, and it never logs it. `Err` says why it was not kept, in the
    /// platform's words.
    pub keep_password: Option<KeepPassword>,
    /// Asks the device's check, showing `reason`, and reads the kept password
    /// back once the check passes. The call returns at once. `answer` is
    /// called once, later, from a thread of the shell's own and never the
    /// interface's. A password is
    /// released only from an item labelled with `identifier`; an item labelled
    /// with another is deleted, and the answer is [`Released::Missing`].
    pub release_password: Option<ReleasePassword>,
    /// Deletes the kept password, whatever its label. `Ok` also when none is
    /// kept. `Err` says why it was not deleted,
    /// in the platform's words.
    pub forget_password: Option<ForgetPassword>,
    /// Starts a QR scan, where a shell has a camera for one. The text the
    /// scan reads comes back through `done`, once (see
    /// [`ScanStart`]). `Err` says why the scan did not start, in the platform's
    /// words: the camera is not allowed, or there is none. `None` where a shell
    /// offers no scan, and on the desktop, which has no `Host` at all.
    pub scan: Option<ScanStart>,
    /// Stops the scan under way, if there is one.
    /// Its camera stops, and its `done` may still be called, with `None`, which
    /// the application drops. With no scan under way it does nothing. It may be
    /// called from any thread: a move to the background calls it from the
    /// shell's own (see [`left_foreground`]).
    pub cancel_scan: Option<fn()>,
    /// Who draws the camera's preview during a scan (see [`ScanPreview`]). An
    /// application offers a scan only when `scan`, `cancel_scan` and this are
    /// all given (see `Scanner::of` in `crate::scan`).
    pub scan_preview: Option<ScanPreview>,
}

/// What [`Host::keep_password`] is.
pub type KeepPassword = fn(identifier: &[u8; 32], password: &str) -> Result<(), String>;

/// What [`Host::scan`] is: starts a scan, whose text `done` receives once. It
/// returns at once, since it runs on the interface's thread: the shell's own
/// scanner is shown and runs on its own. `done` may be called before it returns,
/// as a refusal the shell makes at once is.
pub type ScanStart = fn(done: ScanDone) -> Result<(), String>;

/// The way a scan's text goes back to the interface, called once, from a thread
/// of the shell's own and never the interface's. It takes the bytes the decoder
/// read, or `None` when the scan was cancelled or the camera was not allowed.
/// The shell does not check the bytes: the application trims them, bounds them,
/// and checks them as UTF-8 and as a destination.
pub type ScanDone = Box<dyn FnOnce(Option<Vec<u8>>) + Send>;

/// Who draws the camera's preview during a scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanPreview {
    /// The shell draws it: iOS presents its own scanner, which shows the camera
    /// through the system's preview layer.
    Shell,
    /// The application draws it, from the frames the shell gives through
    /// [`preview_frame`]: Android.
    Application,
}

/// What [`Host::release_password`] is.
pub type ReleasePassword = fn(identifier: &[u8; 32], reason: &str, answer: Deliver);

/// What [`Host::forget_password`] is.
pub type ForgetPassword = fn() -> Result<(), String>;

/// The way a release's answer goes back to the interface: called once, from the
/// shell's thread (see [`ReleasePassword`]).
pub type Deliver = Box<dyn FnOnce(Released) + Send>;

/// What the device's own check is called, as the offer names it. Face ID or
/// Touch ID where the device has one, and
/// otherwise the passcode, which the check accepts on every device that has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceCheck {
    /// Face ID.
    FaceId,
    /// Touch ID.
    TouchId,
    /// The device passcode.
    Passcode,
}

impl DeviceCheck {
    /// What the check is called on the screens: "Face ID", "Touch ID", or
    /// "passcode".
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            DeviceCheck::FaceId => "Face ID",
            DeviceCheck::TouchId => "Touch ID",
            DeviceCheck::Passcode => "passcode",
        }
    }
}

/// What the device's check and the system's storage answered, for
/// [`Host::release_password`].
#[derive(Debug)]
pub enum Released {
    /// The check passed and the password was read back. It is moved into a
    /// [`SecretText`] at once.
    Password(SecretText),
    /// The check did not pass, or the person cancelled it. Nothing changed.
    Declined,
    /// No password is kept under the identifier: none was kept, or the one kept
    /// was labelled with another identifier, and the shell has deleted it.
    Missing,
    /// The password could not be read, for the platform's reason. Nothing
    /// changed.
    Failed(String),
}

/// A [`Released`] on its way from the shell's thread into the interface, with
/// the number of the ask it answers (see [`Device`]). It is `Clone` only because
/// the interface's messages are, so it is taken out once, as a worker's event
/// is. Its `Debug` prints nothing of a password.
#[derive(Clone)]
pub struct Answer(Arc<Mutex<Option<(u64, Released)>>>);

impl Answer {
    /// The answer to ask number `ask`.
    pub(crate) fn new(ask: u64, released: Released) -> Answer {
        Answer(Arc::new(Mutex::new(Some((ask, released)))))
    }

    /// The ask this answers, and the answer: once, then `None`.
    pub(crate) fn take(&self) -> Option<(u64, Released)> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

impl core::fmt::Debug for Answer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Answer(..)")
    }
}

/// What a shell's device unlock gives the application: the device's check, the
/// three functions of [`Host`], and the channel the answers come back on. It
/// is made only where the shell gives all
/// four device fields and the device can offer its check, so that the
/// application never has to ask whether a function is there.
#[derive(Clone, Debug)]
pub struct Device {
    check: DeviceCheck,
    keep: KeepPassword,
    release: ReleasePassword,
    forget: ForgetPassword,
    answers: UnboundedSender<Answer>,
}

impl Device {
    /// The device unlock `host` offers, with its answers sent to `answers`, or
    /// `None` where it offers none. [`App::boot_in`](crate::app::App::boot_in)
    /// makes the one a phone uses; the screenshots make one from a fake `host`
    /// to draw the screens that show it.
    #[must_use]
    pub fn of(host: &Host, answers: UnboundedSender<Answer>) -> Option<Device> {
        let (Some(check), Some(keep), Some(release), Some(forget)) = (
            host.device_check,
            host.keep_password,
            host.release_password,
            host.forget_password,
        ) else {
            return None;
        };
        Some(Device {
            check: check()?,
            keep,
            release,
            forget,
            answers,
        })
    }

    /// What the device's check is called.
    #[must_use]
    pub fn check(&self) -> DeviceCheck {
        self.check
    }

    /// Keeps `password` under `identifier` (see [`Host::keep_password`]).
    pub(crate) fn keep(&self, identifier: &[u8; 32], password: &str) -> Result<(), String> {
        (self.keep)(identifier, password)
    }

    /// Asks the shell for the kept password, as ask number `ask`. Its answer
    /// comes into the interface as an [`Answer`] for that number, so an answer
    /// to an ask the interface has since dropped is recognised and ignored.
    pub(crate) fn ask(&self, identifier: &[u8; 32], reason: &str, ask: u64) {
        let answers = self.answers.clone();
        let deliver: Deliver = Box::new(move |released| {
            // The interface may have closed by now, with nothing left to answer.
            let _ = answers.unbounded_send(Answer::new(ask, released));
        });
        (self.release)(identifier, reason, deliver);
    }

    /// Deletes the kept password (see [`Host::forget_password`]).
    pub(crate) fn forget(&self) -> Result<(), String> {
        (self.forget)()
    }
}

/// What a shell's share sheet is: [`Host::share`].
type Sheet = fn(&str, &str) -> Result<(), String>;

/// The shell's clipboard, share sheet and Back, once a hosted application
/// has started.
static COPY: OnceLock<fn(&str)> = OnceLock::new();
static SHARE: OnceLock<Sheet> = OnceLock::new();
static BACK: OnceLock<fn()> = OnceLock::new();

/// What [`left_foreground`] does: the running worker's lock.
type Lock = Box<dyn Fn() -> Leaving + Send>;

/// A move to the background on its way. A
/// shell whose process the system may suspend keeps it running until
/// [`Leaving::wait`] says the interface has done with the move: taken the
/// worker's `Backgrounded` for it, which comes after the worker has locked
/// and after every answer that came before (a recovery phrase among them,
/// dropped by the lock that follows it); or, with no worker, wiped what is
/// typed. Either way the fields' own wipe on the interface's next message
/// comes first.
#[derive(Debug)]
#[must_use = "a shell that may be suspended waits on it"]
pub struct Leaving(Option<(Arc<Wiped>, Done)>);

/// What the interface must have done for a [`Leaving`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Done {
    /// Taken the worker's `Backgrounded` with this number.
    Taken(u64),
    /// With no worker: wiped for this many moves.
    Wiped(u64),
}

impl Leaving {
    /// Wait up to `within` for the interface to have done with the move.
    /// `true` once it has, or when there was nothing to do; `false` when it
    /// is still waited on after `within`.
    pub fn wait(&self, within: Duration) -> bool {
        self.0
            .as_ref()
            .is_none_or(|(wiped, done)| wiped.wait_for(*done, within))
    }

    pub(crate) fn of(wiped: Option<(Arc<Wiped>, Done)>) -> Leaving {
        Leaving(wiped)
    }
}

/// What the interface has done for the moves to the background: the moves
/// it has wiped what is typed for, and the last of the worker's
/// `Backgrounded` it has taken. The application's thread raises them, and a
/// shell's thread waits on them.
#[derive(Debug, Default)]
pub(crate) struct Wiped {
    state: Mutex<(u64, u64)>,
    raised: Condvar,
}

impl Wiped {
    /// The interface has wiped for `moves` moves.
    pub(crate) fn wiped(&self, moves: u64) {
        self.raise(|state| state.0 = state.0.max(moves));
    }

    /// The interface has taken the worker's `Backgrounded` numbered `seq`,
    /// and so every event before it. `u64::MAX` when the worker has stopped:
    /// nothing more comes from it.
    pub(crate) fn taken(&self, seq: u64) {
        self.raise(|state| state.1 = state.1.max(seq));
    }

    fn raise(&self, change: impl FnOnce(&mut (u64, u64))) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        change(&mut state);
        self.raised.notify_all();
    }

    /// Wait up to `within` for `done`.
    fn wait_for(&self, done: Done, within: Duration) -> bool {
        let reached = |state: &(u64, u64)| match done {
            Done::Taken(seq) => state.1 >= seq,
            Done::Wiped(moves) => state.0 >= moves,
        };
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let (state, _) = self
            .raised
            .wait_timeout_while(state, within, |state| !reached(state))
            .unwrap_or_else(PoisonError::into_inner);
        reached(&state)
    }
}

static LOCK: Mutex<Option<Lock>> = Mutex::new(None);

/// The application has left the foreground: stop what can be stopped and
/// lock, at once, on the calling thread. It does
/// not wait for iced, which may not run again before the system suspends
/// the process, nor for the lock itself: the returned [`Leaving`] says when
/// that is done. Nothing happens before the worker has started.
pub fn left_foreground() -> Leaving {
    match LOCK.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        Some(lock) => lock(),
        None => Leaving::of(None),
    }
}

/// `lock` is what [`left_foreground`] does from now on.
pub(crate) fn on_leaving(lock: impl Fn() -> Leaving + Send + 'static) {
    *LOCK.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(lock));
}

pub(crate) fn set(host: &Host) {
    let _ = COPY.set(host.copy);
    let _ = SHARE.set(host.share);
    let _ = BACK.set(host.back);
}

/// The system's Back, where a shell has one.
pub(crate) fn back() {
    count_back();
    if let Some(back) = BACK.get() {
        back();
    }
}

#[cfg(test)]
thread_local! {
    /// How many times [`back`] was called on this thread, so that a test can
    /// tell whether the system's Back went to the shell.
    static BACKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Counts a call of [`back`], for the tests.
#[cfg(test)]
fn count_back() {
    BACKS.with(|b| b.set(b.get() + 1));
}

/// Counts nothing outside the tests.
#[cfg(not(test))]
fn count_back() {}

/// How many times [`back`] has been called on this thread, for the tests.
#[cfg(test)]
pub(crate) fn backs() -> u32 {
    BACKS.with(std::cell::Cell::get)
}

/// The shell's share sheet, where a shell has one: `None` with no shell, as
/// on the desktop, which saves its file to the Downloads folder instead.
pub(crate) fn share(name: &str, text: &str) -> Option<Result<(), String>> {
    SHARE.get().map(|sheet| sheet(name, text))
}

/// Puts `text` on the shell's clipboard. `false` when there is no shell's,
/// and iced's is the one to use.
pub(crate) fn copy(text: &str) -> bool {
    COPY.get().map(|copy| copy(text)).is_some()
}

/// Where [`screenshot_taken`] and [`capture_changed`] write: the capture state
/// of the hosted application, once it has started.
static CAPTURE: Mutex<Option<Arc<Capture>>> = Mutex::new(None);

/// A screenshot was taken. iOS says so after the capture, through
/// `UIApplicationUserDidTakeScreenshotNotification`, so it cannot block the
/// capture. The application then warns on the recovery phrase's screen, when
/// the words were on it. A screenshot taken
/// before the application has started is not counted.
pub fn screenshot_taken() {
    if let Some(capture) = registered() {
        capture.shot();
    }
}

/// The screen is being recorded or mirrored (`captured`), or no longer is,
/// as iOS reports it through `UIScreenCapturedDidChangeNotification`. The
/// application hides the recovery phrase's words while it is captured. The
/// report is best effort: iOS can read the screen as not captured while a
/// recording runs. A change before the
/// application has started is not kept; [`Host::captured`] gives that state.
pub fn capture_changed(captured: bool) {
    if let Some(capture) = registered() {
        capture.set_captured(captured);
    }
}

/// The capture state [`screenshot_taken`] and [`capture_changed`] write to, if
/// an application has started.
fn registered() -> Option<Arc<Capture>> {
    CAPTURE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// What the shell has reported about the screen's capture. The application
/// reads it on its UI thread, at each message, as it reads the moves to the
/// background: a screenshot is a change in the count, however many came since
/// the last read, and the capture state is the last one the shell gave.
#[derive(Debug, Default)]
pub(crate) struct Capture {
    shots: AtomicU64,
    captured: AtomicBool,
}

impl Capture {
    /// A screenshot was taken.
    pub(crate) fn shot(&self) {
        self.shots.fetch_add(1, Ordering::SeqCst);
    }

    /// The screen is captured (`true`) or no longer is.
    pub(crate) fn set_captured(&self, captured: bool) {
        self.captured.store(captured, Ordering::SeqCst);
    }

    /// How many screenshots have been taken, and whether the screen is
    /// captured now.
    pub(crate) fn read(&self) -> (u64, bool) {
        (
            self.shots.load(Ordering::SeqCst),
            self.captured.load(Ordering::SeqCst),
        )
    }
}

/// Makes `capture` the state [`screenshot_taken`] and [`capture_changed`]
/// write to. Where the shell can read the screen's state, `captured` gives it
/// as the application starts. It is read after `capture` is registered, so a
/// change made in between is not lost.
pub(crate) fn on_capture(capture: Arc<Capture>, captured: Option<fn() -> bool>) {
    *CAPTURE.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&capture));
    if let Some(read) = captured {
        capture.set_captured(read());
    }
}

/// One frame of the camera's preview, as a shell gives it (see [`preview_frame`]).
pub(crate) struct Frame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// The latest preview frame not yet taken.
static FRAME: Mutex<Option<Frame>> = Mutex::new(None);

/// Hands the application the camera's latest preview frame, from the shell's
/// thread. `rgba` holds `width` by `height`
/// pixels, four bytes each, RGBA and not premultiplied, row by row from the top.
/// The application takes a frame at its next redraw, while a scan draws its
/// preview in the application ([`ScanPreview::Application`]). A frame not yet
/// taken is replaced by the next, so the application shows the latest. A frame
/// with no pixels, or whose bytes are not `width` by `height` by four, is
/// dropped.
pub fn preview_frame(width: u32, height: u32, rgba: Vec<u8>) {
    if !frame_fits(width, height, rgba.len()) {
        return;
    }
    *FRAME.lock().unwrap_or_else(PoisonError::into_inner) = Some(Frame {
        width,
        height,
        rgba,
    });
}

/// The latest preview frame, if one has come since the last take.
pub(crate) fn take_frame() -> Option<Frame> {
    FRAME.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// Whether `len` bytes are a frame of `width` by `height` pixels, four bytes
/// each, and the frame has pixels.
fn frame_fits(width: u32, height: u32, len: usize) -> bool {
    width > 0
        && height > 0
        && (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            == Some(len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn leaving_the_foreground_locks_with_whatever_was_registered_last() {
        // Nothing registered: nothing to do, no panic, and nothing to wait
        // for.
        assert!(left_foreground().wait(Duration::ZERO));
        let first = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&first);
        on_leaving(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Leaving::of(None)
        });
        let _ = left_foreground();
        let counter = Arc::clone(&second);
        on_leaving(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Leaving::of(None)
        });
        let _ = left_foreground();
        let _ = left_foreground();
        assert_eq!(first.load(Ordering::SeqCst), 1);
        assert_eq!(second.load(Ordering::SeqCst), 2, "the worker started last");
    }

    #[test]
    fn a_screenshot_is_counted_and_the_capture_state_is_the_last_one_given() {
        let capture = Capture::default();
        assert_eq!(capture.read(), (0, false));
        capture.shot();
        capture.shot();
        capture.set_captured(true);
        assert_eq!(capture.read(), (2, true));
        capture.set_captured(false);
        assert_eq!(capture.read(), (2, false));
    }

    #[test]
    fn the_shell_reaches_the_registered_capture_and_reads_its_start_state() {
        fn captured_at_start() -> bool {
            true
        }
        // Nothing registered: the shell's calls do nothing, and do not panic.
        screenshot_taken();
        capture_changed(false);
        let capture = Arc::new(Capture::default());
        on_capture(Arc::clone(&capture), Some(captured_at_start));
        assert_eq!(capture.read(), (0, true), "the state at start is read");
        screenshot_taken();
        capture_changed(false);
        assert_eq!(capture.read(), (1, false));
    }

    fn copy_nothing(_: &str) {}
    fn share_nothing(_: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn back_nothing() {}
    fn check_face() -> Option<DeviceCheck> {
        Some(DeviceCheck::FaceId)
    }
    fn check_none() -> Option<DeviceCheck> {
        None
    }
    fn keep_nothing(_: &[u8; 32], _: &str) -> Result<(), String> {
        Ok(())
    }
    fn forget_nothing() -> Result<(), String> {
        Ok(())
    }
    fn release_missing(_: &[u8; 32], _: &str, answer: Box<dyn FnOnce(Released) + Send>) {
        answer(Released::Missing);
    }
    /// Answers from a thread of its own, as a shell's prompt does.
    fn release_from_another_thread(
        _: &[u8; 32],
        _: &str,
        answer: Box<dyn FnOnce(Released) + Send>,
    ) {
        std::thread::spawn(move || answer(Released::Declined));
    }

    /// A shell with no device unlock, as Android has it, and no scan.
    fn bare() -> Host {
        Host {
            private_dir: PathBuf::from("/nonexistent"),
            copy: copy_nothing,
            share: share_nothing,
            back: back_nothing,
            notices: include_str!("../../../THIRD-PARTY-NOTICES.txt"),
            captured: None,
            device_check: None,
            keep_password: None,
            release_password: None,
            forget_password: None,
            scan: None,
            cancel_scan: None,
            scan_preview: None,
        }
    }

    #[test]
    fn a_preview_frame_must_have_pixels_and_exactly_four_bytes_each() {
        assert!(frame_fits(2, 3, 2 * 3 * 4));
        assert!(frame_fits(1, 1, 4));
        assert!(!frame_fits(0, 3, 0), "no pixels");
        assert!(!frame_fits(2, 0, 0), "no pixels");
        assert!(!frame_fits(2, 3, 2 * 3 * 3), "three bytes a pixel");
        assert!(!frame_fits(2, 3, 2 * 3 * 4 + 1), "one byte too many");
        assert!(!frame_fits(u32::MAX, u32::MAX, 4), "a size that overflows");
    }

    /// A shell that gives all four device fields, with `check` as its check.
    fn offering(check: fn() -> Option<DeviceCheck>) -> Host {
        Host {
            device_check: Some(check),
            keep_password: Some(keep_nothing),
            release_password: Some(release_missing),
            forget_password: Some(forget_nothing),
            ..bare()
        }
    }

    /// The next answer on `rx`, once it has come, from whichever thread sent it.
    fn next_answer(rx: &mut iced::futures::channel::mpsc::UnboundedReceiver<Answer>) -> Answer {
        use iced::futures::channel::mpsc::TryRecvError;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match rx.try_recv() {
                Ok(answer) => return answer,
                Err(TryRecvError::Closed) => panic!("the answers were closed"),
                Err(TryRecvError::Empty) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(TryRecvError::Empty) => panic!("no answer came"),
            }
        }
    }

    #[test]
    fn a_shell_with_no_device_fields_offers_no_device_unlock() {
        let (answers, _received) = iced::futures::channel::mpsc::unbounded();
        assert!(Device::of(&bare(), answers).is_none());
    }

    #[test]
    fn a_device_that_can_offer_no_check_offers_no_device_unlock() {
        let (answers, _received) = iced::futures::channel::mpsc::unbounded();
        assert!(Device::of(&offering(check_none), answers).is_none());
    }

    #[test]
    fn a_missing_device_field_offers_no_device_unlock() {
        let (answers, _received) = iced::futures::channel::mpsc::unbounded();
        let host = Host {
            keep_password: None,
            ..offering(check_face)
        };
        assert!(Device::of(&host, answers).is_none());
    }

    #[test]
    fn all_four_device_fields_and_a_check_offer_the_device_unlock() {
        let (answers, _received) = iced::futures::channel::mpsc::unbounded();
        let device = Device::of(&offering(check_face), answers).expect("offered");
        assert_eq!(device.check(), DeviceCheck::FaceId);
    }

    #[test]
    fn an_answer_reaches_the_interface_with_the_ask_it_answers() {
        let (answers, mut received) = iced::futures::channel::mpsc::unbounded();
        let device = Device::of(&offering(check_face), answers).expect("offered");
        device.ask(&[1; 32], "unlock", 7);
        let answer = next_answer(&mut received);
        assert!(
            matches!(answer.take(), Some((7, Released::Missing))),
            "the answer of ask 7"
        );
        assert!(answer.take().is_none(), "an answer is taken once");
    }

    #[test]
    fn an_answer_sent_from_another_thread_reaches_the_interface() {
        let (answers, mut received) = iced::futures::channel::mpsc::unbounded();
        let host = Host {
            release_password: Some(release_from_another_thread),
            ..offering(check_face)
        };
        let device = Device::of(&host, answers).expect("offered");
        device.ask(&[2; 32], "unlock", 1);
        let answer = next_answer(&mut received);
        assert!(matches!(answer.take(), Some((1, Released::Declined))));
    }

    #[test]
    fn a_released_password_and_an_answer_print_no_text() {
        let mut text = String::from("correct horse battery");
        let released = Released::Password(SecretText::take(&mut text));
        assert_eq!(format!("{released:?}"), "Password(SecretText(..))");
        let answer = Answer::new(3, Released::Failed("not kept".to_owned()));
        assert_eq!(format!("{answer:?}"), "Answer(..)");
        let (answers, _received) = iced::futures::channel::mpsc::unbounded();
        let device = Device::of(&offering(check_face), answers).expect("offered");
        assert!(
            !format!("{device:?}").contains("correct horse"),
            "the device prints no text"
        );
    }
}
