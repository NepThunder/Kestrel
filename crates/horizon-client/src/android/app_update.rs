//! Kestrel's own update check: on launch, ask GitHub whether a newer
//! Kestrel release exists, and, only when the player taps Download, fetch
//! the APK and hand it to the system installer. Nothing downloads or
//! installs silently, and the check never blocks launch: it runs on a
//! background thread and fails silently when offline.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// The repo whose releases carry Kestrel builds.
pub const REPO: &str = "NepThunder/horizon-android";
/// Release tags are `kestrel-build-<run number>-attempt-<attempt>`.
const TAG_PREFIX: &str = "kestrel-build-";
const APK_NAME: &str = "kestrel-arm64-v8a.apk";
/// The downloaded APK, served to the system installer by the provider.
pub const APK_FILE: &str = "kestrel-update.apk";

/// This build's number, embedded by CI (`KESTREL_BUILD_NUMBER` is the
/// workflow run number); None for a local build, which never offers an
/// update.
pub fn own_build_number() -> Option<u64> {
    option_env!("KESTREL_BUILD_NUMBER")?.parse().ok()
}

/// A newer Kestrel release than the running build.
#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub build: u64,
    pub apk_url: String,
    pub apk_size: u64,
}

fn parse_build(tag: &str) -> Option<u64> {
    let rest = tag.strip_prefix(TAG_PREFIX)?;
    let (num, _) = rest.split_once("-attempt-")?;
    num.parse().ok()
}

fn fetch_releases() -> Result<serde_json::Value, String> {
    use std::io::Read;
    let mut text = String::new();
    ureq::AgentBuilder::new()
        .https_only(true)
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build()
        .get(&format!(
            "https://api.github.com/repos/{REPO}/releases?per_page=100"
        ))
        .set("User-Agent", "Kestrel-Android")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_reader()
        .take(4 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > 4 * 1024 * 1024 {
        return Err("GitHub releases response is too large".into());
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// The newest Kestrel release newer than this build, or None (up to date,
/// offline, or a local build). Blocking: call on a background thread.
pub fn check_latest() -> Option<ReleaseInfo> {
    let own = own_build_number()?;
    let releases = fetch_releases().ok()?;
    let releases = releases.as_array()?;
    let mut best: Option<ReleaseInfo> = None;
    for release in releases {
        let tag = release.get("tag_name")?.as_str()?;
        let build = parse_build(tag)?;
        if build <= own || best.as_ref().is_some_and(|b| b.build >= build) {
            continue;
        }
        let assets = release.get("assets")?.as_array()?;
        let asset = assets
            .iter()
            .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(APK_NAME))?;
        let url = asset.get("browser_download_url")?.as_str()?.to_owned();
        let size = asset.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
        best = Some(ReleaseInfo {
            build,
            apk_url: url,
            apk_size: size,
        });
    }
    best
}

/// Download the release APK to `dest`, reporting 0.0..1.0 progress.
/// Blocking: call on a background thread.
pub fn download_apk(
    url: &str,
    dest: &std::path::Path,
    progress: &dyn Fn(f32),
) -> Result<(), String> {
    use std::io::Read;
    use std::io::Write;
    let response = ureq::AgentBuilder::new()
        .https_only(true)
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(120))
        .build()
        .get(url)
        .set("User-Agent", "Kestrel-Android")
        .call()
        .map_err(|e| e.to_string())?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if total > 0 {
            progress((done as f32 / total as f32).clamp(0.0, 1.0));
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

/// The prompt's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// No check running and none offered.
    Idle,
    /// The background check is running.
    Checking,
    /// A newer build is offered; the player has not acted yet.
    Available,
    /// The offered APK is downloading.
    Downloading,
    /// The APK is downloaded; the player can install it.
    Ready,
    /// The download failed; the player can retry or dismiss.
    Failed,
    /// The player dismissed the offer this launch.
    Dismissed,
}

/// The app-update prompt, owned by the launcher.
pub struct AppUpdate {
    state: State,
    release: Option<ReleaseInfo>,
    downloaded: Option<PathBuf>,
    error: Option<String>,
    check: Option<mpsc::Receiver<Option<ReleaseInfo>>>,
    download: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    progress: Arc<Mutex<f32>>,
    /// Build the player dismissed with Later; the launch check will not
    /// offer it again. A manual check ignores this.
    dismissed_build: Option<u64>,
    /// Whether the check currently in flight was started by hand.
    manual: bool,
    /// Outcome of the last manual check: Some(true) found an update,
    /// Some(false) found none. None until a manual check completes.
    manual_outcome: Option<bool>,
}

impl AppUpdate {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            release: None,
            downloaded: None,
            error: None,
            check: None,
            download: None,
            progress: Arc::new(Mutex::new(0.0)),
            dismissed_build: None,
            manual: false,
            manual_outcome: None,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn release(&self) -> Option<&ReleaseInfo> {
        self.release.as_ref()
    }

    pub fn downloaded(&self) -> Option<&PathBuf> {
        self.downloaded.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn progress(&self) -> f32 {
        *self.progress.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The build the player dismissed with Later, if any.
    pub fn dismissed_build(&self) -> Option<u64> {
        self.dismissed_build
    }

    /// Restore the dismissed build (loaded from the launcher config).
    pub fn set_dismissed_build(&mut self, build: Option<u64>) {
        self.dismissed_build = build;
    }

    /// Outcome of the last manual check: Some(true) found an update,
    /// Some(false) found none. None until a manual check completes.
    pub fn manual_outcome(&self) -> Option<bool> {
        self.manual_outcome
    }

    /// Start the launch check (once; a no-op for a local build, which has no
    /// build number embedded). The check runs on a background thread and
    /// never blocks launch.
    pub fn start_check(&mut self) {
        self.begin_check(false);
    }

    /// Start a manual check, e.g. from the "Check for updates" button.
    /// Unlike the launch check, it runs even after the player dismissed an
    /// offer, and it offers the dismissed build again.
    pub fn recheck(&mut self) {
        self.begin_check(true);
    }

    fn begin_check(&mut self, manual: bool) {
        if !matches!(
            self.state,
            State::Idle | State::Dismissed | State::Failed
        ) || self.check.is_some()
        {
            return;
        }
        if own_build_number().is_none() {
            return;
        }
        self.state = State::Checking;
        self.manual = manual;
        self.manual_outcome = None;
        let (tx, rx) = mpsc::channel();
        self.check = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(check_latest());
        });
    }

    /// Poll the background check and download; call once per frame.
    pub fn poll(&mut self) {
        if let Some(rx) = &self.check
            && let Ok(found) = rx.try_recv()
        {
            self.check = None;
            let manual = std::mem::replace(&mut self.manual, false);
            if matches!(self.state, State::Checking) {
                if manual {
                    self.manual_outcome = Some(found.is_some());
                }
                match found {
                    Some(info) => {
                        if !manual && self.dismissed_build == Some(info.build) {
                            // Dismissed with Later: do not offer this build
                            // again on launch. A manual check still shows it.
                            self.release = None;
                            self.state = State::Idle;
                        } else {
                            self.release = Some(info);
                            self.state = State::Available;
                        }
                    }
                    None => {
                        self.release = None;
                        self.state = State::Idle;
                    }
                }
            }
        }
        if let Some(rx) = &self.download
            && let Ok(result) = rx.try_recv()
        {
            self.download = None;
            if matches!(self.state, State::Downloading) {
                match result {
                    Ok(path) => {
                        self.downloaded = Some(path);
                        self.state = State::Ready;
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.state = State::Failed;
                    }
                }
            }
        }
    }

    /// Start downloading the offered release (only from Available or Failed).
    pub fn start_download(&mut self, cache_dir: &std::path::Path) {
        if !matches!(self.state, State::Available | State::Failed) {
            return;
        }
        let Some(info) = self.release.clone() else {
            return;
        };
        self.state = State::Downloading;
        self.error = None;
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = 0.0;
        let progress = self.progress.clone();
        let url = info.apk_url.clone();
        let dest = cache_dir.join(APK_FILE);
        let (tx, rx) = mpsc::channel();
        self.download = Some(rx);
        std::thread::spawn(move || {
            let result = download_apk(&url, &dest, &|p| {
                *progress.lock().unwrap_or_else(|e| e.into_inner()) = p;
            })
            .map(|()| dest);
            let _ = tx.send(result);
        });
    }

    /// Dismiss the offer for this launch. A download already running is
    /// abandoned; its partial file is overwritten by the next download.
    /// The offered build is remembered so the launch check does not offer
    /// it again; a manual check still offers it.
    pub fn dismiss(&mut self) {
        if let Some(info) = &self.release {
            self.dismissed_build = Some(info.build);
        }
        self.state = State::Dismissed;
        self.check = None;
        self.download = None;
    }
}

impl Default for AppUpdate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_tag_build_parses() {
        assert_eq!(parse_build("kestrel-build-18-attempt-1"), Some(18));
        assert_eq!(parse_build("kestrel-build-101-attempt-2"), Some(101));
        assert_eq!(parse_build("debug-build-8-attempt-1"), None);
        assert_eq!(parse_build("kestrel-build-18"), None);
        assert_eq!(parse_build(""), None);
    }

    #[test]
    fn local_build_has_no_build_number() {
        // `KESTREL_BUILD_NUMBER` is only set by CI; a local build (this
        // test) has none, so the check must stay off.
        assert_eq!(own_build_number(), None);
    }

    fn release(build: u64) -> ReleaseInfo {
        ReleaseInfo {
            build,
            apk_url: "https://example.com/kestrel-arm64-v8a.apk".into(),
            apk_size: 1,
        }
    }

    /// Drive a check result into `poll` without touching the network.
    fn with_check_result(update: &mut AppUpdate, manual: bool, found: Option<ReleaseInfo>) {
        let (tx, rx) = mpsc::channel();
        update.state = State::Checking;
        update.manual = manual;
        update.check = Some(rx);
        tx.send(found).unwrap();
        update.poll();
    }

    #[test]
    fn dismiss_records_the_offered_build() {
        let mut update = AppUpdate::new();
        update.release = Some(release(21));
        update.state = State::Available;
        update.dismiss();
        assert_eq!(update.dismissed_build(), Some(21));
        assert_eq!(update.state(), State::Dismissed);
    }

    #[test]
    fn auto_check_skips_the_dismissed_build() {
        let mut update = AppUpdate::new();
        update.dismissed_build = Some(21);
        with_check_result(&mut update, false, Some(release(21)));
        assert_eq!(update.state(), State::Idle);
        assert!(update.release().is_none());
        assert_eq!(update.manual_outcome(), None);
    }

    #[test]
    fn auto_check_still_offers_a_newer_build() {
        let mut update = AppUpdate::new();
        update.dismissed_build = Some(21);
        with_check_result(&mut update, false, Some(release(22)));
        assert_eq!(update.state(), State::Available);
        assert_eq!(update.release().unwrap().build, 22);
    }

    #[test]
    fn manual_check_offers_the_dismissed_build() {
        let mut update = AppUpdate::new();
        update.dismissed_build = Some(21);
        with_check_result(&mut update, true, Some(release(21)));
        assert_eq!(update.state(), State::Available);
        assert_eq!(update.release().unwrap().build, 21);
        assert_eq!(update.manual_outcome(), Some(true));
    }

    #[test]
    fn manual_check_with_no_update_reports_up_to_date() {
        let mut update = AppUpdate::new();
        with_check_result(&mut update, true, None);
        assert_eq!(update.state(), State::Idle);
        assert_eq!(update.manual_outcome(), Some(false));
    }
}
