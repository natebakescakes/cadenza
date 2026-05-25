// Serial device layer: a Rust port of the CharaChorder `DeviceManager`
// `CharaDevice` (TS) using the `serialport` crate.
//
// Connection: baud 921600. Commands are ASCII terminated with `\r\n`; each
// response is a single line that ECHOES the command followed by space-separated
// fields. We strip the echoed prefix and split on spaces.
//
// Protocol:
//   VERSION                   -> [version]
//   ID                        -> [company, device, chipset]
//   CML C0                    -> [count]
//   CML C1 <idx>              -> [actionsHex, phraseHex]
//   CML C2 <actionsHex>       -> [phraseHex] (or "2" = not found)
//   CML C3 <actionsHex> <phraseHex> -> [status] (0 = ok)
//   CML C4 <actionsHex>       -> delete
//
// Action codec: 10-bit codes, max 12, packed big-end into a u128 rendered as
// 32 uppercase hex chars. Phrase codec: variable-length 8/13-bit ints.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serialport::{SerialPort, SerialPortType};

use crate::types::{DeviceInfo, DeviceSettings, SerialPortInfo};

const BAUD: u32 = 921_600;
/// Per-command read timeout.
const CMD_TIMEOUT: Duration = Duration::from_secs(2);

// --- VID/PID -> friendly name -------------------------------------------------

/// Known CharaChorder USB (vid, pid) -> internal alias.
const PORT_FILTERS: &[(u16, u16, &str)] = &[
    (0x2386, 0x800F, "ONE M0"),               // 9114 / 32783
    (0x303A, 0x8252, "TWO S3 (pre-production)"),
    (0x303A, 0x8253, "TWO S3"),
    (0x303A, 0x812E, "LITE S2"),
    (0x2386, 0x801C, "LITE M0"),              // 9114 / 32796
    (0x303A, 0x818B, "X"),
    (0x303A, 0x1001, "M4G S3 (pre-production)"),
    (0x303A, 0x829A, "M4G S3"),
    (0x303A, 0x82F2, "CCB S2"),
];

/// Maps an internal alias to a friendly display name (mirrors DEVICE_ALIASES).
fn friendly_name(alias: &str) -> &'static str {
    match alias {
        "ONE M0" => "CC1",
        "TWO S3" | "TWO S3 (pre-production)" => "CC2",
        "LITE S2" => "Lite (S2)",
        "LITE M0" => "Lite (M0)",
        "X" => "CCX",
        "M4G S3" | "M4G S3 (pre-production)" => "M4G",
        "M4GR S3" => "M4G (right)",
        "CCB S2" => "CCB",
        _ => "CharaChorder",
    }
}

/// Looks up a friendly name for a (vid, pid), or `None` if unrecognized.
fn name_for_vid_pid(vid: u16, pid: u16) -> Option<&'static str> {
    PORT_FILTERS
        .iter()
        .find(|(v, p, _)| *v == vid && *p == pid)
        .map(|(_, _, alias)| friendly_name(alias))
}

/// Whether a (vid, pid) looks like a CharaChorder device (CC vendor IDs).
fn is_charachorder_vendor(vid: u16) -> bool {
    vid == 0x303A || vid == 0x2386
}

// --- Action codec (10-bit, max 12, packed into u128 -> 32 hex chars) ----------

/// Packs up to 12 10-bit action codes into a u128 (mirrors `serializeActions`).
fn serialize_actions(actions: &[u16]) -> u128 {
    let mut native: u128 = 0;
    let len = actions.len();
    for i in 1..=len {
        let a = (actions[len - i] & 0x3ff) as u128;
        native |= a << ((12 - i) * 10);
    }
    native
}

/// Unpacks a u128 into 12 10-bit action codes (mirrors `deserializeActions`).
fn deserialize_actions(mut native: u128) -> Vec<u16> {
    let mut actions = Vec::with_capacity(12);
    for _ in 0..12 {
        actions.push((native & 0x3ff) as u16);
        native >>= 10;
    }
    actions
}

/// `<actionsHex>` (32 uppercase hex) -> action codes.
fn parse_chord_actions(hex: &str) -> Result<Vec<u16>> {
    let native = u128::from_str_radix(hex.trim(), 16)
        .map_err(|e| anyhow!("bad actions hex '{hex}': {e}"))?;
    Ok(deserialize_actions(native))
}

/// action codes -> `<actionsHex>` (32 uppercase hex, mirrors `stringifyChordActions`).
fn stringify_chord_actions(actions: &[u16]) -> String {
    format!("{:032X}", serialize_actions(actions))
}

// --- Phrase codec (variable-length 8/13-bit ints) -----------------------------

/// Decompresses raw bytes into action codes (mirrors `decompressActions`).
fn decompress_actions(raw: &[u8]) -> Vec<u16> {
    let mut actions = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let mut action = raw[i] as u16;
        if raw[i] > 0 && raw[i] < 32 && i + 1 < raw.len() {
            i += 1;
            action = (action << 8) | raw[i] as u16;
        }
        actions.push(action);
        i += 1;
    }
    actions
}

/// Compresses action codes into variable-length bytes (mirrors `compressActions`).
fn compress_actions(actions: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(actions.len() * 2);
    for &action in actions {
        if action > 0xff {
            out.push((action >> 8) as u8);
        }
        out.push((action & 0xff) as u8);
    }
    out
}

/// `<phraseHex>` -> action codes (mirrors `parsePhrase`).
fn parse_phrase(hex: &str) -> Result<Vec<u16>> {
    let hex = hex.trim();
    if hex.len() % 2 != 0 {
        return Err(anyhow!("odd-length phrase hex"));
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let b = u8::from_str_radix(&hex[i..i + 2], 16)
            .map_err(|e| anyhow!("bad phrase hex: {e}"))?;
        bytes.push(b);
    }
    Ok(decompress_actions(&bytes))
}

/// action codes -> `<phraseHex>` (mirrors `stringifyPhrase`).
fn stringify_phrase(actions: &[u16]) -> String {
    compress_actions(actions)
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect()
}

/// Renders decoded phrase action codes into a human-readable string.
///
/// Action codes in printable ASCII range (32..=126) map directly to that char.
/// Non-printable / special codes are encoded as `\u{XXXX}` escapes so the
/// phrase round-trips deterministically without crashing on control codes.
fn phrase_to_string(codes: &[u16]) -> String {
    let mut s = String::new();
    for &c in codes {
        if (32..=126).contains(&c) {
            s.push(c as u8 as char);
        } else if c == 0 {
            // padding / empty action: skip
            continue;
        } else {
            s.push_str(&format!("\\u{{{:04X}}}", c));
        }
    }
    s
}

// --- Device -------------------------------------------------------------------

/// Owns an open serial connection to a CharaChorder device.
pub struct Device {
    port: Box<dyn SerialPort>,
    /// Leftover bytes read past a newline (buffered for the next read_line).
    buf: Vec<u8>,
    info: DeviceInfo,
}

impl Device {
    /// Opens the serial port at 921600 baud and probes VERSION/ID/C0.
    pub fn connect(port_name: &str) -> Result<(Self, DeviceInfo)> {
        let port = serialport::new(port_name, BAUD)
            .timeout(CMD_TIMEOUT)
            .open()
            .map_err(|e| anyhow!("failed to open port '{port_name}': {e}"))?;

        let mut dev = Device {
            port,
            buf: Vec::new(),
            info: DeviceInfo {
                name: String::new(),
                company: String::new(),
                device: String::new(),
                chipset: String::new(),
                version: String::new(),
                port: port_name.to_string(),
                chord_count: 0,
            },
        };

        let info = dev.init(port_name)?;
        Ok((dev, info))
    }

    /// Probes VERSION + ID + chord count and fills `self.info`.
    fn init(&mut self, port_name: &str) -> Result<DeviceInfo> {
        let version = self
            .send(&["VERSION"])?
            .into_iter()
            .next()
            .unwrap_or_default();

        let id = self.send(&["ID"])?;
        let company = id.first().cloned().unwrap_or_default();
        let device = id.get(1).cloned().unwrap_or_default();
        let chipset = id.get(2).cloned().unwrap_or_default();

        let chord_count = self.get_chord_count().unwrap_or(0);

        // Prefer a USB VID/PID friendly name; fall back to the ID device field.
        let name = port_friendly_name(port_name)
            .map(|n| n.to_string())
            .unwrap_or_else(|| {
                if !device.is_empty() {
                    device.clone()
                } else {
                    "CharaChorder".to_string()
                }
            });

        self.info = DeviceInfo {
            name,
            company,
            device,
            chipset,
            version,
            port: port_name.to_string(),
            chord_count,
        };
        Ok(self.info.clone())
    }

    /// A stable identifier for this device used as the `device_id` in storage.
    pub fn device_id(&self) -> String {
        format!("{}-{}", self.info.name, self.info.version)
    }

    /// Reads one `\n`-terminated line (trimmed), respecting `CMD_TIMEOUT`.
    fn read_line(&mut self) -> Result<String> {
        let deadline = Instant::now() + CMD_TIMEOUT;
        loop {
            // Emit a complete line already in the buffer.
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=pos).collect();
                let s = String::from_utf8_lossy(&line);
                return Ok(s.trim_matches(|c| c == '\r' || c == '\n').to_string());
            }
            if Instant::now() >= deadline {
                return Err(anyhow!("read timeout waiting for device response"));
            }
            let mut chunk = [0u8; 256];
            match self.port.read(&mut chunk) {
                Ok(0) => continue,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    return Err(anyhow!("read timeout waiting for device response"));
                }
                Err(e) => return Err(anyhow!("serial read error: {e}")),
            }
        }
    }

    /// Sends a command and returns its space-separated response fields (with the
    /// echoed command prefix stripped).
    pub fn send(&mut self, command: &[&str]) -> Result<Vec<String>> {
        let cmd = command.join(" ");
        self.buf.clear();
        self.port
            .write_all(format!("{cmd}\r\n").as_bytes())
            .map_err(|e| anyhow!("serial write error: {e}"))?;
        self.port.flush().ok();

        let line = self.read_line()?;
        // Strip the echoed command prefix ("CMD ...") if present.
        let body = line.strip_prefix(&format!("{cmd} ")).unwrap_or(&line);
        Ok(body.split(' ').map(|s| s.to_string()).collect())
    }

    /// `CML C0` -> total chord count.
    pub fn get_chord_count(&mut self) -> Result<i64> {
        let resp = self.send(&["CML", "C0"])?;
        let count = resp
            .first()
            .and_then(|c| c.trim().parse::<i64>().ok())
            .ok_or_else(|| anyhow!("bad chord count response"))?;
        Ok(count)
    }

    /// `CML C1 <index>` -> (raw decoded action-code bytes, decoded phrase string).
    ///
    /// The returned `Vec<u8>` is the compressed phrase-action bytes (the same
    /// bytes that round-trip via the phrase codec), stored as the BLOB.
    pub fn get_chord(&mut self, index: i64) -> Result<(Vec<u8>, String)> {
        let resp = self.send(&["CML", "C1", &index.to_string()])?;
        let actions_hex = resp.first().cloned().unwrap_or_default();
        let phrase_hex = resp.get(1).cloned().unwrap_or_default();

        // Actions (the chord input) -> codes -> stored as their hex string bytes.
        let action_codes = parse_chord_actions(&actions_hex)?;
        let phrase_codes = parse_phrase(&phrase_hex)?;
        let phrase = phrase_to_string(&phrase_codes);

        // Store the raw action-code bytes (compressed form) as the BLOB so it is
        // self-describing and round-trippable.
        let blob = compress_actions(&action_codes);
        Ok((blob, phrase))
    }

    /// `CML C2 <actionsHex>` -> phrase action codes, or `None` if not found ("2").
    #[allow(dead_code)]
    pub fn get_chord_phrase(&mut self, actions: &[u16]) -> Result<Option<Vec<u16>>> {
        let resp = self.send(&["CML", "C2", &stringify_chord_actions(actions)])?;
        match resp.first().map(|s| s.as_str()) {
            Some("2") => Ok(None),
            Some(hex) => Ok(Some(parse_phrase(hex)?)),
            None => Ok(None),
        }
    }

    /// `CML C3 <actionsHex> <phraseHex>` -> define a chord. (Defined; not wired.)
    #[allow(dead_code)]
    pub fn set_chord(&mut self, actions: &[u16], phrase: &[u16]) -> Result<()> {
        let resp = self.send(&[
            "CML",
            "C3",
            &stringify_chord_actions(actions),
            &stringify_phrase(phrase),
        ])?;
        match resp.last().map(|s| s.as_str()) {
            Some("0") => Ok(()),
            other => Err(anyhow!("set_chord failed with status {:?}", other)),
        }
    }

    /// `CML C4 <actionsHex>` -> delete a chord. (Defined; not wired.)
    #[allow(dead_code)]
    pub fn delete_chord(&mut self, actions: &[u16]) -> Result<()> {
        let resp = self.send(&["CML", "C4", &stringify_chord_actions(actions)])?;
        match resp.last().map(|s| s.as_str()) {
            Some("0") | Some("2") => Ok(()),
            other => Err(anyhow!("delete_chord failed with status {:?}", other)),
        }
    }

    /// `VAR B1 <idHex>` — read a single device setting (profile 0).
    ///
    /// The response echoes the command; the value is the last space-separated
    /// field. Returns `Err` on timeout or parse failure — never panics.
    pub fn get_setting(&mut self, id: u16) -> Result<i64> {
        let id_hex = format!("{:02X}", id);
        let resp = self.send(&["VAR", "B1", &id_hex])?;
        // Response after echo-strip is "<value> <status>" (status 0 = ok), or just
        // "<value>". The VALUE precedes the trailing status — taking the LAST field
        // grabbed the status (always 0 on success), which made every setting read 0.
        if resp.is_empty() {
            return Err(anyhow!("empty VAR B1 response for id {}", id_hex));
        }
        let val_str = if resp.len() >= 2 {
            &resp[resp.len() - 2]
        } else {
            &resp[0]
        };
        val_str
            .trim()
            .parse::<i64>()
            .map_err(|e| anyhow!("VAR B1 {}: bad value '{}': {}", id_hex, val_str, e))
    }

    /// Query all Cadenza-relevant device settings in one pass.
    ///
    /// On per-field failure the sentinel value -1 is stored and querying
    /// continues — a partial result is always returned, never Err.
    pub fn read_device_settings(&mut self) -> DeviceSettings {
        let get = |dev: &mut Device, id: u16| -> i64 {
            dev.get_setting(id).unwrap_or(-1)
        };

        let output_delay_us           = get(self, 0x17);
        let arpeggiate_timeout_ms_raw = get(self, 0x54);
        let arpeggiate_enabled_raw    = get(self, 0x51);
        let chord_press_tolerance_ms  = get(self, 0x34);
        let chord_release_tolerance_ms= get(self, 0x35);
        let auto_delete_timeout_ms    = get(self, 0x33);
        let chording_enabled_raw      = get(self, 0x31);
        let spurring_enabled_raw      = get(self, 0x41);

        DeviceSettings {
            output_delay_us,
            arpeggiate_timeout_ms: arpeggiate_timeout_ms_raw,
            arpeggiate_enabled:    arpeggiate_enabled_raw == 1,
            chord_press_tolerance_ms,
            chord_release_tolerance_ms,
            auto_delete_timeout_ms,
            chording_enabled:      chording_enabled_raw == 1,
            spurring_enabled:      spurring_enabled_raw == 1,
        }
    }

    /// `VAR B3 A1 <id>` → action code assigned to that key position on profile 0, layer 1.
    /// Returns 0 on failure (unset/out-of-range positions are fine to skip).
    pub fn get_layout_key(&mut self, id: u16) -> u16 {
        let resp = self.send(&["VAR", "B3", "A1", &id.to_string()]);
        match resp {
            Ok(fields) if fields.len() >= 2 && fields[1].trim() == "0" => {
                fields[0].trim().parse::<u16>().unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Reads all key positions 0..90 for profile 0, layer 1.
    /// Returns Vec<(position, action_code)> for non-zero entries only.
    pub fn read_layout(&mut self) -> Vec<(u16, u16)> {
        let mut out = Vec::new();
        for id in 0u16..90 {
            let code = self.get_layout_key(id);
            if code != 0 {
                out.push((id, code));
            }
        }
        out
    }

    /// Walks the whole chord map, returning `(phrase, actions_blob)` pairs.
    pub fn read_all_chords(&mut self) -> Result<Vec<(String, Vec<u8>)>> {
        let count = self.get_chord_count()?;
        let mut out = Vec::with_capacity(count.max(0) as usize);
        for i in 0..count {
            match self.get_chord(i) {
                Ok((blob, phrase)) => {
                    if !phrase.is_empty() {
                        out.push((phrase, blob));
                    }
                }
                // Skip individual malformed entries rather than aborting the sync.
                Err(_) => continue,
            }
        }
        Ok(out)
    }
}

/// Friendly name for a port by matching its USB VID/PID against known devices.
fn port_friendly_name(port_name: &str) -> Option<&'static str> {
    let ports = serialport::available_ports().ok()?;
    for p in ports {
        if p.port_name == port_name {
            if let SerialPortType::UsbPort(usb) = p.port_type {
                return name_for_vid_pid(usb.vid, usb.pid);
            }
        }
    }
    None
}

/// Enumerates serial ports, keeping known CharaChorder VID/PIDs (and any
/// CC-vendor ports as "Unknown CharaChorder").
pub fn scan_devices() -> Vec<SerialPortInfo> {
    let ports = match serialport::available_ports() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for p in ports {
        if let SerialPortType::UsbPort(usb) = &p.port_type {
            if let Some(name) = name_for_vid_pid(usb.vid, usb.pid) {
                out.push(SerialPortInfo {
                    port: p.port_name.clone(),
                    name: name.to_string(),
                });
            } else if is_charachorder_vendor(usb.vid) {
                out.push(SerialPortInfo {
                    port: p.port_name.clone(),
                    name: "Unknown CharaChorder".to_string(),
                });
            }
        }
    }
    out
}
