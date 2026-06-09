use anyhow::{anyhow, bail, Context, Result};
use plist::Value;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PAGE_SIZE: usize = 4096;
const RESERVED_OFFSET: usize = 4084;
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const WAL_HEADER_SIZE: usize = 32;
const WAL_FRAME_HEADER_SIZE: usize = 24;

#[derive(Clone, Debug)]
pub struct FindMyConfig {
    pub key_path: PathBuf,
    pub db_path: PathBuf,
}

impl FindMyConfig {
    pub fn new(key_path: PathBuf, db_path: Option<PathBuf>) -> Result<Self> {
        let db_path = match db_path {
            Some(path) => path,
            None => default_db_path()?,
        };

        Ok(Self { key_path, db_path })
    }
}

#[derive(Debug)]
pub struct ExtractKeyResult {
    pub key_path: PathBuf,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct FriendLocation {
    pub handle: String,
    pub display_name: String,
    pub initial: String,
    pub contact_id: Option<i64>,
    pub has_photo: bool,
    pub photo_url: String,
    pub latitude: f64,
    pub longitude: f64,
    pub horizontal_accuracy: f64,
    pub accuracy_label: String,
    pub altitude: f64,
    pub speed: f64,
    pub course: f64,
    pub timestamp: String,
    pub relative_time: String,
    pub motion_state: String,
    pub location_label: String,
    pub findmy_id: String,
}

pub fn default_db_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(
        "Library/Group Containers/group.com.apple.findmy.findmylocateagent/Library/Application Support/LocalStorage.db",
    ))
}

pub fn default_key_path() -> PathBuf {
    PathBuf::from("data/findmy/LocalStorage.key")
}

pub fn default_config_from_saved_key() -> Result<Option<FindMyConfig>> {
    let key_path = default_key_path();
    if key_path.exists() {
        Ok(Some(FindMyConfig::new(key_path, None)?))
    } else {
        Ok(None)
    }
}

pub fn extract_localstorage_key(key_path: &Path, db_path: &Path) -> Result<ExtractKeyResult> {
    if !cfg!(target_os = "macos") {
        bail!("Find My key extraction is only available on macOS");
    }

    if Command::new("lldb")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        bail!(
            "lldb is not available. Install Xcode Command Line Tools with xcode-select --install"
        );
    }

    let parent = key_path
        .parent()
        .ok_or_else(|| anyhow!("invalid Find My key path {}", key_path.display()))?;
    fs::create_dir_all(parent)?;
    let _ = fs::remove_file(key_path);

    let script_path = parent.join("extract_localstorage_key.py");
    fs::write(&script_path, extraction_script(key_path)?)?;

    let child = Command::new("lldb")
        .arg("--wait-for")
        .arg("-n")
        .arg("findmylocateagent")
        .arg("-o")
        .arg("settings set frame-format ''")
        .arg("-o")
        .arg("settings set auto-confirm true")
        .arg("-o")
        .arg(format!("command script import {}", script_path.display()))
        .arg("-o")
        .arg("c")
        .arg("-o")
        .arg("quit")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start lldb")?;

    std::thread::sleep(Duration::from_secs(1));
    restart_findmy_agent();

    let output = wait_with_timeout(child, Duration::from_secs(35))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !key_path.exists() {
        let details = [stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "LocalStorage.key was not captured. macOS usually requires SIP disabled and AMFI boot args disabled for lldb to attach to findmylocateagent.{}{}",
            if details.is_empty() { "" } else { "\n\nlldb output:\n" },
            details
        );
    }

    let key_len = fs::metadata(key_path)?.len();
    if key_len != 32 {
        bail!("captured key has invalid length: {key_len} bytes");
    }

    let config = FindMyConfig::new(key_path.to_path_buf(), Some(db_path.to_path_buf()))?;
    read_friend_locations(&config).context("captured key could not decrypt Find My database")?;

    Ok(ExtractKeyResult {
        key_path: key_path.to_path_buf(),
        message: format!("Saved and verified {}", key_path.display()),
    })
}

pub fn read_friend_locations(config: &FindMyConfig) -> Result<Vec<FriendLocation>> {
    let key = fs::read(&config.key_path)
        .with_context(|| format!("failed to read key at {}", config.key_path.display()))?;
    if key.len() != 32 {
        bail!("Find My key must be 32 bytes, got {}", key.len());
    }

    if !config.db_path.exists() {
        bail!("Find My database not found at {}", config.db_path.display());
    }

    let pages = merge_pages(&config.db_path, &key)?;
    let tmp = write_merged_db(&pages)?;
    let result = query_locations(&tmp);
    let _ = fs::remove_file(&tmp);
    result
}

fn merge_pages(db_path: &Path, key: &[u8]) -> Result<BTreeMap<u32, Vec<u8>>> {
    let raw_db = fs::read(db_path)
        .with_context(|| format!("failed to read Find My database at {}", db_path.display()))?;
    let mut pages = BTreeMap::new();

    for (idx, chunk) in raw_db.chunks(PAGE_SIZE).enumerate() {
        if chunk.len() == PAGE_SIZE {
            let page_number = idx as u32 + 1;
            pages.insert(page_number, decrypt_page(chunk, page_number, key)?);
        }
    }

    let first = pages
        .get(&1)
        .ok_or_else(|| anyhow!("Find My database is empty"))?;
    if first.get(..16) != Some(SQLITE_MAGIC) {
        bail!("Find My key verification failed");
    }

    let wal_path = db_path.with_file_name(format!(
        "{}-wal",
        db_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("LocalStorage.db")
    ));

    if !wal_path.exists() {
        return Ok(pages);
    }

    let wal = fs::read(&wal_path)
        .with_context(|| format!("failed to read Find My WAL at {}", wal_path.display()))?;
    if wal.len() < WAL_HEADER_SIZE {
        return Ok(pages);
    }

    let wal_page_size = u32::from_be_bytes(wal[8..12].try_into().unwrap()) as usize;
    if wal_page_size != PAGE_SIZE {
        bail!("unsupported Find My WAL page size {wal_page_size}");
    }

    let frame_size = WAL_FRAME_HEADER_SIZE + wal_page_size;
    let frame_count = (wal.len() - WAL_HEADER_SIZE) / frame_size;
    for idx in 0..frame_count {
        let frame_offset = WAL_HEADER_SIZE + idx * frame_size;
        let page_number =
            u32::from_be_bytes(wal[frame_offset..frame_offset + 4].try_into().unwrap());
        let page_start = frame_offset + WAL_FRAME_HEADER_SIZE;
        let page_end = page_start + wal_page_size;
        pages.insert(
            page_number,
            decrypt_page(&wal[page_start..page_end], page_number, key)?,
        );
    }

    Ok(pages)
}

fn write_merged_db(pages: &BTreeMap<u32, Vec<u8>>) -> Result<PathBuf> {
    let mut merged = Vec::with_capacity(pages.len() * PAGE_SIZE);
    for page in pages.values() {
        merged.extend_from_slice(page);
    }

    if merged.len() > 19 {
        merged[18] = 1;
        merged[19] = 1;
    }

    let path = std::env::temp_dir().join(format!(
        "imessage-view-findmy-{}-{}.db",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::write(&path, merged)?;
    Ok(path)
}

fn extraction_script(key_path: &Path) -> Result<String> {
    let key_path = serde_json::to_string(&key_path.to_string_lossy())?;
    Ok(format!(
        r#"
import lldb
from pathlib import Path

KEY_FILE = Path({key_path})
_done = False
_bp = None

def _is_x86(frame):
    triple = frame.GetThread().GetProcess().GetTarget().GetTriple() or ""
    return triple.startswith("x86_64")

def _arg(frame, n):
    names = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"] if _is_x86(frame) else ["x0", "x1", "x2", "x3", "x4", "x5"]
    return frame.FindRegister(names[n]).GetValueAsUnsigned()

def _read_mem(process, ptr, size):
    if not ptr or size <= 0 or size > 65536:
        return None
    err = lldb.SBError()
    data = process.ReadMemory(int(ptr), int(size), err)
    return bytes(data) if err.Success() else None

def _get_db_path(frame, db_ptr):
    global _bp
    if _bp and _bp.IsValid():
        _bp.SetEnabled(False)
    opts = lldb.SBExpressionOptions()
    opts.SetTimeoutInMicroSeconds(5_000_000)
    opts.SetTryAllThreads(False)
    result = frame.EvaluateExpression('(char*)sqlite3_db_filename((void*)%d, "main")' % db_ptr, opts)
    if _bp and _bp.IsValid():
        _bp.SetEnabled(True)
    if result.GetError().Fail():
        return None
    ptr = result.GetValueAsUnsigned()
    if not ptr:
        return None
    data = _read_mem(frame.GetThread().GetProcess(), ptr, 512)
    if not data:
        return None
    return data.split(b"\x00")[0].decode("utf-8", errors="replace")

def on_sqlite3_key_v2(frame, bp_loc, extra_args, internal_dict):
    global _done
    if _done:
        return False
    process = frame.GetThread().GetProcess()
    db_ptr = _arg(frame, 0)
    key_ptr = _arg(frame, 2)
    key_len = _arg(frame, 3)
    print("sqlite3_key_v2 hit len=%d" % key_len, flush=True)
    if key_len != 32:
        return False
    db_path = _get_db_path(frame, db_ptr)
    if not db_path or "LocalStorage" not in db_path:
        return False
    key_data = _read_mem(process, key_ptr, 32)
    if not key_data:
        print("failed to read key memory", flush=True)
        return False
    KEY_FILE.parent.mkdir(parents=True, exist_ok=True)
    KEY_FILE.write_bytes(key_data)
    _done = True
    print("captured LocalStorage key", flush=True)
    process.Kill()
    return False

def __lldb_init_module(debugger, internal_dict):
    global _bp
    target = debugger.GetSelectedTarget()
    if not target or not target.IsValid():
        print("no valid target", flush=True)
        return
    _bp = target.BreakpointCreateByName("sqlite3_key_v2")
    _bp.SetScriptCallbackFunction("extract_localstorage_key.on_sqlite3_key_v2")
    print("intercepting sqlite3_key_v2", flush=True)
"#
    ))
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output> {
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child
                .wait_with_output()
                .context("failed to collect lldb output");
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .context("failed to collect lldb output")?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "timed out waiting for lldb to capture LocalStorage.key.{}{}{}{}",
                if stdout.trim().is_empty() {
                    ""
                } else {
                    "\nstdout:\n"
                },
                stdout.trim(),
                if stderr.trim().is_empty() {
                    ""
                } else {
                    "\nstderr:\n"
                },
                stderr.trim()
            );
        }

        std::thread::sleep(Duration::from_millis(250));
    }
}

fn restart_findmy_agent() {
    if let Ok(uid_output) = Command::new("id").arg("-u").output() {
        if uid_output.status.success() {
            if let Ok(uid) = String::from_utf8(uid_output.stdout) {
                let domain = format!("gui/{}/com.apple.findmy.findmylocateagent", uid.trim());
                let _ = Command::new("launchctl")
                    .arg("kickstart")
                    .arg("-k")
                    .arg(domain)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }

    let _ = Command::new("open")
        .arg("-gja")
        .arg("FindMy")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn query_locations(db_path: &Path) -> Result<Vec<FriendLocation>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT f.handleIdentifier, sl.value
        FROM friends f
        LEFT JOIN secureLocations sl ON sl.serverUserID = f.handleServerIdentifier
        ORDER BY f.handleIdentifier
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        let handle: String = row.get(0)?;
        let blob: Option<Vec<u8>> = row.get(1)?;
        Ok((handle, blob))
    })?;

    let mut locations = Vec::new();
    for row in rows {
        let (handle, blob) = row?;
        if let Some(blob) = blob {
            if let Ok(location) = parse_location(&handle, &blob) {
                locations.push(location);
            }
        }
    }

    Ok(locations)
}

fn parse_location(handle: &str, blob: &[u8]) -> Result<FriendLocation> {
    let value = Value::from_reader(Cursor::new(blob))?;
    let dict = value
        .as_dictionary()
        .ok_or_else(|| anyhow!("Find My location blob is not a dictionary"))?;

    let latitude = value_f64(dict.get("latitude")).context("missing latitude")?;
    let longitude = value_f64(dict.get("longitude")).context("missing longitude")?;
    if !(latitude >= -90.0 && latitude <= 90.0 && longitude >= -180.0 && longitude <= 180.0) {
        bail!("invalid Find My coordinates");
    }
    if latitude == 0.0 && longitude == 0.0 {
        bail!("empty Find My coordinates");
    }

    let horizontal_accuracy = value_f64(dict.get("horizontalAccuracy")).unwrap_or(0.0);
    let altitude = value_f64(dict.get("altitude")).unwrap_or(0.0);
    let speed = value_f64(dict.get("speed")).unwrap_or(-1.0);
    let course = value_f64(dict.get("course")).unwrap_or(-1.0);
    let motion_activity_state = value_i64(dict.get("motionActivityState")).unwrap_or(0);
    let location_label = display_location_label(
        &value_string(dict.get("locationLabel")).unwrap_or_default(),
        latitude,
        longitude,
    );
    let findmy_id = value_string(dict.get("findMyId")).unwrap_or_default();
    let timestamp = value_timestamp(dict.get("timestamp"));
    let motion_state = if speed > 5.0 || location_label.to_lowercase().contains("driv") {
        "Driving".to_string()
    } else if motion_activity_state == 0 {
        "Stationary".to_string()
    } else {
        "Moving".to_string()
    };

    let display_name = crate::web::format::format_contact_value(handle);

    Ok(FriendLocation {
        handle: handle.to_string(),
        initial: crate::web::format::display_initial(&display_name),
        display_name,
        contact_id: None,
        has_photo: false,
        photo_url: String::new(),
        latitude,
        longitude,
        horizontal_accuracy,
        accuracy_label: format_accuracy(horizontal_accuracy),
        altitude,
        speed,
        course,
        timestamp: timestamp.to_rfc3339(),
        relative_time: relative_time(timestamp.timestamp()),
        motion_state,
        location_label,
        findmy_id,
    })
}

fn format_accuracy(value: f64) -> String {
    if value >= 1000.0 {
        format!("{:.1} km", value / 1000.0)
    } else {
        format!("{value:.0} m")
    }
}

fn value_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Real(value) => Some(*value),
        Value::Integer(value) => value.as_signed().map(|value| value as f64),
        _ => None,
    }
}

fn value_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Integer(value) => value.as_signed(),
        Value::Real(value) => Some(*value as i64),
        _ => None,
    }
}

fn value_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn value_timestamp(value: Option<&Value>) -> chrono::DateTime<chrono::Utc> {
    match value {
        Some(Value::Date(value)) => {
            let system_time: std::time::SystemTime = (*value).into();
            chrono::DateTime::<chrono::Utc>::from(system_time)
        }
        Some(Value::Real(value)) => {
            chrono::DateTime::from_timestamp((978_307_200.0 + value) as i64, 0)
                .unwrap_or_else(chrono::Utc::now)
        }
        Some(Value::Integer(value)) => value
            .as_signed()
            .and_then(|value| chrono::DateTime::from_timestamp(978_307_200 + value, 0))
            .unwrap_or_else(chrono::Utc::now),
        _ => chrono::Utc::now(),
    }
}

fn display_location_label(value: &str, latitude: f64, longitude: f64) -> String {
    match value.trim() {
        "_$!!$_" => "Home".to_string(),
        "$null" | "(null)" | "null" | "NULL" | "<null>" | "None" | "none" => {
            coordinate_label(latitude, longitude)
        }
        "" => coordinate_label(latitude, longitude),
        _ => value.to_string(),
    }
}

fn coordinate_label(latitude: f64, longitude: f64) -> String {
    format!("{latitude:.5}, {longitude:.5}")
}

fn relative_time(unix: i64) -> String {
    let diff = chrono::Utc::now().timestamp() - unix;
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

#[cfg(target_os = "macos")]
fn decrypt_page(enc_page: &[u8], page_number: u32, key: &[u8]) -> Result<Vec<u8>> {
    use std::ffi::{c_int, c_uint};

    const K_CC_ENCRYPT: c_uint = 0;
    const K_CC_ALGORITHM_AES: c_uint = 0;
    const K_CC_OPTION_PKCS7_PADDING: c_uint = 1;
    const K_CC_SUCCESS: c_int = 0;
    const K_CC_BLOCK_SIZE_AES128: usize = 16;

    #[link(name = "System", kind = "dylib")]
    unsafe extern "C" {
        fn CCCrypt(
            op: c_uint,
            alg: c_uint,
            options: c_uint,
            key: *const c_void,
            key_length: usize,
            iv: *const c_void,
            data_in: *const c_void,
            data_in_length: usize,
            data_out: *mut c_void,
            data_out_available: usize,
            data_out_moved: *mut usize,
        ) -> c_int;
    }

    if enc_page.len() != PAGE_SIZE {
        bail!(
            "Find My page must be {PAGE_SIZE} bytes, got {}",
            enc_page.len()
        );
    }

    let mut iv = [0u8; 16];
    iv[0..4].copy_from_slice(&page_number.to_le_bytes());
    iv[4..16].copy_from_slice(&enc_page[RESERVED_OFFSET..PAGE_SIZE]);

    let zeros = vec![0u8; PAGE_SIZE];
    let mut keystream = vec![0u8; PAGE_SIZE + K_CC_BLOCK_SIZE_AES128];
    let mut keystream_len = 0usize;
    let status = unsafe {
        CCCrypt(
            K_CC_ENCRYPT,
            K_CC_ALGORITHM_AES,
            K_CC_OPTION_PKCS7_PADDING,
            key.as_ptr().cast(),
            key.len(),
            iv.as_ptr().cast(),
            zeros.as_ptr().cast(),
            zeros.len(),
            keystream.as_mut_ptr().cast(),
            keystream.len(),
            &mut keystream_len,
        )
    };
    if status != K_CC_SUCCESS {
        bail!("CommonCrypto AES failed with status {status}");
    }

    let mut plaintext = vec![0u8; RESERVED_OFFSET];
    for idx in 0..RESERVED_OFFSET {
        plaintext[idx] = enc_page[idx] ^ keystream[idx];
    }

    if page_number == 1 {
        plaintext[16..24].copy_from_slice(&enc_page[16..24]);
    }

    plaintext.extend_from_slice(&enc_page[RESERVED_OFFSET..PAGE_SIZE]);
    Ok(plaintext)
}

#[cfg(not(target_os = "macos"))]
fn decrypt_page(_enc_page: &[u8], _page_number: u32, _key: &[u8]) -> Result<Vec<u8>> {
    bail!("Find My database decryption is only implemented on macOS")
}
