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

fn load_contacts_from_db(
    db_path: &PathBuf,
    contacts: &mut HashMap<String, ContactInfo>,
) -> anyhow::Result<()> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let mut name_map: HashMap<i64, String> = HashMap::new();
    let mut photo_map: HashMap<i64, Vec<u8>> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT Z_PK, ZFIRSTNAME, ZLASTNAME, ZTHUMBNAILIMAGEDATA FROM ZABCDRECORD")?;
        let rows = stmt.query_map([], |row| {
            let pk: i64 = row.get(0)?;
            let first: Option<String> = row.get(1)?;
            let last: Option<String> = row.get(2)?;
            let photo: Option<Vec<u8>> = row.get(3)?;
            Ok((pk, first, last, photo))
        })?;
        for row in rows.flatten() {
            let (pk, first, last, photo) = row;
            let display = format_name(first.as_deref(), last.as_deref());
            if !display.is_empty() {
                name_map.insert(pk, display);
            }
            if let Some(bytes) = photo {
                if let Some(image_data) = extract_image_data(bytes) {
                    photo_map.insert(pk, image_data);
                }
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
                        contacts.insert(
                            normalized,
                            ContactInfo {
                                display_name: name.clone(),
                                photo: photo_map.get(&owner_id).cloned(),
                            },
                        );
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
                        contacts.insert(
                            normalized,
                            ContactInfo {
                                display_name: name.clone(),
                                photo: photo_map.get(&owner_id).cloned(),
                            },
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

fn extract_image_data(raw: Vec<u8>) -> Option<Vec<u8>> {
    if raw.is_empty() {
        return None;
    }
    match raw[0] {
        0x01 if raw.len() > 1 => Some(raw[1..].to_vec()),
        0x02 => None,
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
