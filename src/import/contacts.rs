use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};
use tracing::warn;

pub struct ContactInfo {
    pub display_name: String,
    pub photo: Option<Vec<u8>>,
}

pub fn resolve_contacts() -> HashMap<String, ContactInfo> {
    let mut contacts: HashMap<String, ContactInfo> = HashMap::new();

    let db_paths = match find_addressbook_dbs() {
        Ok(paths) => paths,
        Err(e) => {
            warn!("Could not search for AddressBook databases: {e}");
            return contacts;
        }
    };

    if db_paths.is_empty() {
        warn!("No AddressBook databases found");
        return contacts;
    }

    for db_path in &db_paths {
        if let Err(e) = load_contacts_from_db(db_path, &mut contacts) {
            warn!("Could not read AddressBook at {}: {e}", db_path.display());
        }
    }

    let nickname_photos = load_imessage_nickname_photos();
    if !nickname_photos.is_empty() {
        eprintln!(
            "Loaded {} iMessage profile photos from NickNameCache",
            nickname_photos.len()
        );
        for (handle, photo_bytes) in nickname_photos {
            let normalized = if handle.contains('@') {
                normalize_email(&handle)
            } else {
                normalize_phone(&handle)
            };
            if normalized.is_empty() {
                continue;
            }
            contacts
                .entry(normalized)
                .and_modify(|existing| {
                    existing.photo = Some(photo_bytes.clone());
                })
                .or_insert(ContactInfo {
                    display_name: String::new(),
                    photo: Some(photo_bytes),
                });
        }
    }

    contacts
}

fn find_addressbook_dbs() -> Result<Vec<PathBuf>, std::io::Error> {
    let home =
        std::env::var("HOME").map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;
    let base = PathBuf::from(&home).join("Library/Application Support/AddressBook");
    let mut paths = Vec::new();

    let direct = base.join("AddressBook-v22.abcddb");
    if direct.exists() {
        paths.push(direct);
    }

    let pattern = base.join("Sources/*/AddressBook-v22.abcddb");
    let pattern_str = pattern.to_string_lossy().to_string();
    match glob::glob(&pattern_str) {
        Ok(entries) => {
            for entry in entries.flatten() {
                paths.push(entry);
            }
        }
        Err(e) => {
            warn!("Glob pattern error for AddressBook sources: {e}");
        }
    }

    Ok(paths)
}

fn load_imessage_nickname_photos() -> HashMap<String, Vec<u8>> {
    let mut result: HashMap<String, Vec<u8>> = HashMap::new();

    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return result,
    };

    let cache_dir = PathBuf::from(&home).join("Library/Messages/NickNameCache");
    let db_path = cache_dir.join("nicknameRecordsStore.db");
    if !db_path.exists() {
        return result;
    }

    let conn = match Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(c) => c,
        Err(e) => {
            warn!("Could not open NickNameCache DB: {e}");
            return result;
        }
    };

    let plist_bytes: Vec<u8> = match conn.query_row(
        "SELECT value FROM kvtable WHERE key = 'activeNicknameRecords'",
        [],
        |r| r.get(0),
    ) {
        Ok(b) => b,
        Err(e) => {
            warn!("Could not read activeNicknameRecords: {e}");
            return result;
        }
    };

    let plist_val: plist::Value = match plist::from_bytes(&plist_bytes) {
        Ok(v) => v,
        Err(e) => {
            warn!("Could not parse NickNameCache plist: {e}");
            return result;
        }
    };

    let objects = match plist_val
        .as_dictionary()
        .and_then(|d| d.get("$objects"))
        .and_then(|v| v.as_array())
    {
        Some(arr) => arr,
        None => return result,
    };

    // Find the NSMutableDictionary containing NS.keys + NS.objects
    for obj in objects {
        let dict = match obj.as_dictionary() {
            Some(d) if d.contains_key("NS.keys") && d.contains_key("NS.objects") => d,
            _ => continue,
        };

        let keys = match dict.get("NS.keys").and_then(|v| v.as_array()) {
            Some(k) => k,
            None => continue,
        };
        let vals = match dict.get("NS.objects").and_then(|v| v.as_array()) {
            Some(v) => v,
            None => continue,
        };

        for (k, v) in keys.iter().zip(vals.iter()) {
            let k_uid = match k.as_uid() {
                Some(u) => u.get() as usize,
                None => continue,
            };
            let v_uid = match v.as_uid() {
                Some(u) => u.get() as usize,
                None => continue,
            };

            let handle = match objects.get(k_uid).and_then(|o| o.as_string()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let avatar_id = match objects.get(v_uid).and_then(|o| o.as_string()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            let avatar_path = cache_dir.join(format!("{avatar_id}-ad"));
            match std::fs::read(&avatar_path) {
                Ok(bytes) if !bytes.is_empty() => {
                    result.insert(handle, bytes);
                }
                _ => {}
            }
        }

        break;
    }

    result
}

fn load_contacts_from_db(
    db_path: &PathBuf,
    contacts: &mut HashMap<String, ContactInfo>,
) -> anyhow::Result<()> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let external_data_dir = db_path
        .parent()
        .map(|p| p.join(".AddressBook-v22_SUPPORT/_EXTERNAL_DATA"));

    let mut name_map: HashMap<i64, String> = HashMap::new();
    let mut photo_map: HashMap<i64, Vec<u8>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT Z_PK, ZFIRSTNAME, ZLASTNAME, ZTHUMBNAILIMAGEDATA, ZIMAGEDATA FROM ZABCDRECORD",
        )?;
        let rows = stmt.query_map([], |row| {
            let pk: i64 = row.get(0)?;
            let first: Option<String> = row.get(1)?;
            let last: Option<String> = row.get(2)?;
            let thumbnail: Option<Vec<u8>> = row.get(3)?;
            let full_image: Option<Vec<u8>> = row.get(4)?;
            Ok((pk, first, last, thumbnail, full_image))
        })?;
        for row in rows.flatten() {
            let (pk, first, last, thumbnail, full_image) = row;
            let display = format_name(first.as_deref(), last.as_deref());
            if !display.is_empty() {
                name_map.insert(pk, display);
            }
            let image = thumbnail
                .and_then(|b| extract_image_data(b, &external_data_dir))
                .or_else(|| full_image.and_then(|b| extract_image_data(b, &external_data_dir)));
            if let Some(image_data) = image {
                photo_map.insert(pk, image_data);
            }
        }
    }

    {
        let mut stmt = conn.prepare("SELECT ZADDRESSNORMALIZED, ZOWNER FROM ZABCDEMAILADDRESS")?;
        let rows = stmt.query_map([], |row| {
            let email: Option<String> = row.get(0)?;
            let owner: Option<i64> = row.get(1)?;
            Ok((email, owner))
        })?;
        for row in rows.flatten() {
            let (email, owner) = row;
            if let (Some(email), Some(owner_id)) = (email, owner) {
                if let Some(name) = name_map.get(&owner_id) {
                    let normalized = normalize_email(&email);
                    if !normalized.is_empty() {
                        let new_photo = photo_map.get(&owner_id).cloned();
                        contacts
                            .entry(normalized)
                            .and_modify(|existing| {
                                existing.display_name = name.clone();
                                if new_photo.is_some() {
                                    existing.photo = new_photo.clone();
                                }
                            })
                            .or_insert(ContactInfo {
                                display_name: name.clone(),
                                photo: new_photo,
                            });
                    }
                }
            }
        }
    }

    {
        let mut stmt = conn.prepare("SELECT ZFULLNUMBER, ZOWNER FROM ZABCDPHONENUMBER")?;
        let rows = stmt.query_map([], |row| {
            let phone: Option<String> = row.get(0)?;
            let owner: Option<i64> = row.get(1)?;
            Ok((phone, owner))
        })?;
        for row in rows.flatten() {
            let (phone, owner) = row;
            if let (Some(phone), Some(owner_id)) = (phone, owner) {
                if let Some(name) = name_map.get(&owner_id) {
                    let normalized = normalize_phone(&phone);
                    if !normalized.is_empty() {
                        let new_photo = photo_map.get(&owner_id).cloned();
                        contacts
                            .entry(normalized)
                            .and_modify(|existing| {
                                existing.display_name = name.clone();
                                if new_photo.is_some() {
                                    existing.photo = new_photo.clone();
                                }
                            })
                            .or_insert(ContactInfo {
                                display_name: name.clone(),
                                photo: new_photo,
                            });
                    }
                }
            }
        }
    }

    Ok(())
}

fn extract_image_data(raw: Vec<u8>, external_data_dir: &Option<PathBuf>) -> Option<Vec<u8>> {
    if raw.is_empty() {
        return None;
    }
    match raw[0] {
        0x01 if raw.len() > 1 => Some(raw[1..].to_vec()),
        0x02 if raw.len() > 1 => {
            let uuid_str = String::from_utf8_lossy(&raw[1..])
                .trim_matches(|c: char| c.is_whitespace() || c == '\0')
                .to_string();
            if uuid_str.is_empty() {
                return None;
            }
            if let Some(dir) = external_data_dir {
                let path = dir.join(&uuid_str);
                match std::fs::read(&path) {
                    Ok(bytes) if !bytes.is_empty() => Some(bytes),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => Some(raw),
    }
}

fn format_name(first: Option<&str>, last: Option<&str>) -> String {
    match (first, last) {
        (Some(f), Some(l)) if !f.is_empty() && !l.is_empty() => format!("{f} {l}"),
        (Some(f), _) if !f.is_empty() => f.to_string(),
        (_, Some(l)) if !l.is_empty() => l.to_string(),
        _ => String::new(),
    }
}

fn normalize_phone(phone: &str) -> String {
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 11 && digits.starts_with('1') {
        digits[1..].to_string()
    } else {
        digits
    }
}

fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_phone_plain() {
        assert_eq!(normalize_phone("5551234567"), "5551234567");
    }

    #[test]
    fn test_normalize_phone_formatted() {
        assert_eq!(normalize_phone("(555) 123-4567"), "5551234567");
    }

    #[test]
    fn test_normalize_phone_plus_one() {
        assert_eq!(normalize_phone("+1 555-123-4567"), "5551234567");
    }

    #[test]
    fn test_normalize_phone_with_one_prefix() {
        assert_eq!(normalize_phone("15551234567"), "5551234567");
    }

    #[test]
    fn test_normalize_email() {
        assert_eq!(normalize_email("  User@Example.COM  "), "user@example.com");
    }

    #[test]
    fn test_format_name_both() {
        assert_eq!(format_name(Some("John"), Some("Doe")), "John Doe");
    }

    #[test]
    fn test_format_name_first_only() {
        assert_eq!(format_name(Some("John"), None), "John");
    }

    #[test]
    fn test_format_name_last_only() {
        assert_eq!(format_name(None, Some("Doe")), "Doe");
    }

    #[test]
    fn test_format_name_empty() {
        assert_eq!(format_name(None, None), "");
    }

    #[test]
    fn test_resolve_contacts_no_panic() {
        let map = resolve_contacts();
        for (_handle, info) in &map {
            let _ = &info.display_name;
            let _ = &info.photo;
        }
    }
}
