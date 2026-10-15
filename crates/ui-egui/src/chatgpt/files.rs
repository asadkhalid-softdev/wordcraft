//! Read explicitly referenced files on the transport worker without replacing the live document.
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

const MAX_CHUNK: usize = 32_768;
const MAX_DOCUMENT: u64 = 32 * 1024 * 1024;

/// Only the user's own chat text grants a path; document/tool content never grants file access.
pub(super) fn authorize<'a>(path: &str, mut messages: impl Iterator<Item = &'a str>, document: Option<&Path>) -> Result<Value, String> {
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err("Provide a valid file path referenced in your chat message".into());
    }
    if !messages.any(|message| mentions_path(message, path)) {
        return Err("The user must reference this file path in a chat message before it can be read".into());
    }
    let supplied = PathBuf::from(path);
    let resolved = if supplied.is_absolute() {
        supplied
    } else {
        document
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .map(Ok)
            .unwrap_or_else(std::env::current_dir)
            .map_err(|_| "Could not resolve the file path")?
            .join(supplied)
    };
    let resolved = resolved.to_str().ok_or("File path is not valid Unicode")?;
    wordcraft_engine::file_access::check_read(resolved)?;
    Ok(json!({"path": resolved}))
}

fn mentions_path(message: &str, path: &str) -> bool {
    #[cfg(windows)]
    let (message, path) = (message.to_lowercase(), path.to_lowercase());
    #[cfg(not(windows))]
    let (message, path) = (message.to_owned(), path.to_owned());
    message.match_indices(&path).any(|(start, _)| {
        let before = message.get(..start).and_then(|s| s.chars().next_back());
        let after = message.get(start.saturating_add(path.len())..).and_then(|s| s.chars().next());
        let boundary = |c: char| c.is_whitespace() || "\"'`<>[](){},;!?".contains(c);
        before.is_none_or(boundary) && after.is_none_or(boundary)
    })
}

pub(super) fn read(authorization: &Value, args: &Value, cancelled: &AtomicBool) -> Result<Value, String> {
    check_cancelled(cancelled)?;
    let path = authorization.get("path").and_then(Value::as_str).ok_or("File access was not authorized")?;
    wordcraft_engine::file_access::check_read(path)?;
    // Reject devices, pipes and directories before opening; recheck the actual opened handle too.
    let metadata = std::fs::metadata(path).map_err(|_| "Could not access the referenced file")?;
    if !metadata.is_file() {
        return Err("The referenced path must be a regular file".into());
    }
    let mut file = std::fs::File::open(path).map_err(|_| "Could not open the referenced file; check the path and operating-system permissions")?;
    let metadata = file.metadata().map_err(|_| "Could not inspect the referenced file")?;
    if !metadata.is_file() {
        return Err("The referenced path must be a regular file".into());
    }
    let offset = args.get("offset").map(|v| v.as_u64().ok_or("File offset must be a nonnegative integer")).transpose()?.unwrap_or(0);
    let limit = args
        .get("limit")
        .map(|v| v.as_u64().ok_or("File limit must be a nonnegative integer"))
        .transpose()?
        .unwrap_or(MAX_CHUNK as u64)
        .clamp(4, MAX_CHUNK as u64) as usize;
    let mode = args.get("mode").and_then(Value::as_str).unwrap_or("auto");
    if !["auto", "text", "base64"].contains(&mode) {
        return Err("File mode must be auto, text or base64".into());
    }
    let extension = Path::new(path).extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if mode == "auto" && ["docx", "docm", "dotx", "dotm", "doc", "dot", "odt", "rtf"].contains(&extension.as_str()) {
        if metadata.len() > MAX_DOCUMENT {
            return Err("Document exceeds the 32 MB text-extraction limit. Use base64 mode for paged raw bytes.".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_DOCUMENT.saturating_add(1)).read_to_end(&mut bytes).map_err(|_| "Could not read the referenced document")?;
        if bytes.len() as u64 > MAX_DOCUMENT {
            return Err("Referenced document grew beyond the text-extraction limit".into());
        }
        check_cancelled(cancelled)?;
        let doc = wordcraft_engine::io::open_bytes(path, &bytes)?;
        let text = doc.plain_text(wordcraft_doc::StoryRef::Body);
        let start =
            usize::try_from(offset).ok().filter(|n| *n <= text.len() && text.is_char_boundary(*n)).ok_or("Invalid extracted-text byte offset")?;
        let mut end = start.saturating_add(limit).min(text.len());
        while !text.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        check_cancelled(cancelled)?;
        return Ok(
            json!({"path": path, "encoding": "text", "format": extension, "offsetUnit": "extracted UTF-8 bytes", "offset": start, "nextOffset": end, "total": text.len(), "text": text.get(start..end).unwrap_or_default(), "hasMore": end < text.len()}),
        );
    }
    if offset > metadata.len() {
        return Err("File offset is beyond the end of the file".into());
    }
    let mut sample = vec![0u8; 4096];
    let sampled = file.read(&mut sample).map_err(|_| "Could not read the referenced file")?;
    sample.truncate(sampled);
    let utf16 = sample.starts_with(&[0xff, 0xfe]) || sample.starts_with(&[0xfe, 0xff]);
    let utf8 = !sample.contains(&0)
        && match std::str::from_utf8(&sample) {
            Ok(_) => true,
            Err(e) => e.error_len().is_none(),
        };
    file.seek(SeekFrom::Start(offset)).map_err(|_| "Could not seek in the referenced file")?;
    let mut bytes = Vec::new();
    file.take(limit as u64).read_to_end(&mut bytes).map_err(|_| "Could not read the referenced file")?;
    let binary = mode == "base64" || (mode == "auto" && !utf8 && !utf16);
    let (encoding, content) = if binary {
        ("base64", json!({"data": STANDARD.encode(&bytes), "note": "Raw file bytes, not extracted text"}))
    } else if utf16 {
        if !metadata.len().is_multiple_of(2) {
            return Err("UTF-16 file has an incomplete code unit. Use base64 mode for raw bytes.".into());
        }
        if !offset.is_multiple_of(2) {
            return Err("UTF-16 file offsets must be even".into());
        }
        bytes.truncate(bytes.len() - bytes.len() % 2);
        let little = sample.starts_with(&[0xff, 0xfe]);
        if offset.saturating_add(bytes.len() as u64) < metadata.len() && bytes.len() >= 2 {
            let tail = bytes.get(bytes.len().saturating_sub(2)..).unwrap_or_default();
            let unit = tail.first().zip(tail.get(1)).map(|(a, b)| if little { u16::from_le_bytes([*a, *b]) } else { u16::from_be_bytes([*a, *b]) });
            if unit.is_some_and(|unit| (0xd800..=0xdbff).contains(&unit)) {
                bytes.truncate(bytes.len().saturating_sub(2));
            }
        }
        let units = bytes.as_chunks::<2>().0.iter().map(|[a, b]| if little { u16::from_le_bytes([*a, *b]) } else { u16::from_be_bytes([*a, *b]) });
        let text: String = char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect();
        ("text", json!({"text": text.trim_start_matches('\u{feff}')}))
    } else {
        if let Err(error) = std::str::from_utf8(&bytes)
            && error.error_len().is_none()
            && error.valid_up_to() > 0
        {
            bytes.truncate(error.valid_up_to());
        }
        ("text", json!({"text": String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}')}))
    };
    check_cancelled(cancelled)?;
    let next = offset.saturating_add(bytes.len() as u64);
    let mut result = json!({"path": path, "encoding": encoding, "offsetUnit": "file bytes", "offset": offset, "nextOffset": next, "total": metadata.len(), "hasMore": next < metadata.len()});
    if let (Some(result), Some(content)) = (result.as_object_mut(), content.as_object()) {
        result.extend(content.clone());
    }
    Ok(result)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) { Err("File read cancelled".into()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn referenced_files_are_authorized_paged_and_never_confused_with_a_directory_grant() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("notes with spaces.txt");
        std::fs::write(&path, "Café 🦀 notes").unwrap();
        let supplied = path.to_str().unwrap();
        assert!(authorize(supplied, ["No file provided"].into_iter(), None).is_err());
        let message = format!("Read `{supplied}`");
        let allowed = authorize(supplied, [message.as_str()].into_iter(), None).unwrap();
        let cancelled = AtomicBool::new(false);
        let first = read(&allowed, &json!({"limit": 5}), &cancelled).unwrap();
        assert_eq!(first["text"], "Café");
        assert_eq!(first["nextOffset"], 5);
        let next = read(&allowed, &json!({"offset": first["nextOffset"]}), &cancelled).unwrap();
        assert_eq!(next["text"], " 🦀 notes");
        assert_eq!(next["hasMore"], false);
        assert!(read(&allowed, &json!({"offset": u64::MAX}), &cancelled).is_err());
        assert!(read(&allowed, &json!({"offset": -1}), &cancelled).is_err());
        assert!(read(&allowed, &json!({"mode": "shell"}), &cancelled).is_err());
        cancelled.store(true, Ordering::Relaxed);
        assert!(read(&allowed, &json!({}), &cancelled).is_err());
        let folder = temp.path().to_str().unwrap();
        assert!(authorize(folder, [message.as_str()].into_iter(), None).is_err());
        assert!(read(&json!({"path": folder}), &json!({}), &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn binary_utf16_and_rich_documents_are_read_without_a_live_session() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("arbitrary.data");
        std::fs::write(&binary, [0, 255, 17, 128, 9]).unwrap();
        let result = read(&json!({"path": binary}), &json!({}), &AtomicBool::new(false)).unwrap();
        assert_eq!(result["encoding"], "base64");
        assert_eq!(STANDARD.decode(result["data"].as_str().unwrap()).unwrap(), vec![0, 255, 17, 128, 9]);
        let utf16 = temp.path().join("utf16.txt");
        let bytes: Vec<u8> = [0xff, 0xfe].into_iter().chain("Café".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        std::fs::write(&utf16, bytes).unwrap();
        let result = read(&json!({"path": utf16}), &json!({}), &AtomicBool::new(false)).unwrap();
        assert_eq!(result["text"], "Café");
        std::fs::write(&utf16, [0xff, 0xfe, 0]).unwrap();
        assert!(read(&json!({"path": utf16}), &json!({}), &AtomicBool::new(false)).is_err());
        let raw = read(&json!({"path": utf16}), &json!({"mode": "base64"}), &AtomicBool::new(false)).unwrap();
        assert_eq!(raw["nextOffset"], 3);
        let rtf = temp.path().join("letter.rtf");
        std::fs::write(&rtf, br"{\rtf1\ansi Hello document}").unwrap();
        let result = read(&json!({"path": rtf}), &json!({}), &AtomicBool::new(false)).unwrap();
        assert!(result["text"].as_str().unwrap().contains("Hello document"));
        assert_eq!(result["offsetUnit"], "extracted UTF-8 bytes");
        let supplied = "notes.txt";
        let message = "Read `notes.txt`";
        let authorization = authorize(supplied, [message].into_iter(), Some(&temp.path().join("document.docx"))).unwrap();
        assert_eq!(PathBuf::from(authorization["path"].as_str().unwrap()), temp.path().join(supplied));
    }
}
