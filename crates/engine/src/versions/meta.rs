//! What a version records about how and where it was saved, without tracking a person.
//!
//! Only one thing goes into the document itself: a random document id ([`DOC_ID_PROP`], a
//! UUID v4 that derives from nothing: not the machine, the user, the time or the path). The
//! history's own records stay in the history folder and hold counts and hashes, never text of
//! the document. The computer appears as a random installation id and a label the user can
//! change ("Windows", "Mac" by default), never its host name or the account name; paths are
//! kept relative to the user's folder or the cloud provider's folder.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// The custom document property (ECMA-376 Part 1 §22.3, `docProps/custom.xml`) that holds the
/// document id. Excel keeps custom properties when it saves (they are the Custom tab of
/// Advanced Properties), so the id survives a round trip through Excel.
pub(crate) const DOC_ID_PROP: &str = "GridCraftDocumentId";
/// The installation id's file in the settings folder.
const INSTALLATION_FILE: &str = "installation-id";

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A new document id: a random UUID (version 4, RFC 9562 §5.4), lower-case.
pub(crate) fn new_document_id() -> Result<String, String> {
    let mut b = random_bytes::<16>()?;
    if let Some(byte) = b.get_mut(6) {
        *byte = (*byte & 0x0f) | 0x40;
    }
    if let Some(byte) = b.get_mut(8) {
        *byte = (*byte & 0x3f) | 0x80;
    }
    let h = hex(&b);
    let part = |range: std::ops::Range<usize>| h.get(range).unwrap_or_default();
    Ok(format!("{}-{}-{}-{}-{}", part(0..8), part(8..12), part(12..16), part(16..20), part(20..32)))
}

/// Is `text` a document id as GridCraft writes them (a UUID in its usual text form)? Ids come
/// from files, so anything else is ignored.
pub(crate) fn document_id_ok(text: &str) -> bool {
    text.len() == 36
        && text
            .bytes()
            .enumerate()
            .all(|(i, b)| if matches!(i, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_hexdigit() && !b.is_ascii_uppercase() })
}

/// The history folder of a document id: its 32 hex digits.
pub(crate) fn id_bucket(id: &str) -> String {
    id.chars().filter(|c| *c != '-').collect()
}

/// The document id in a workbook package, read from its custom properties without opening the
/// workbook. `None` for anything that is not a package with a valid id.
pub(crate) fn package_document_id<R: Read + std::io::Seek>(reader: R) -> Option<String> {
    gridcraft_xlsx::custom_property_in(reader, DOC_ID_PROP).map(|value| value.trim().to_owned()).filter(|id| document_id_ok(id))
}

/// The document id of the workbook package at `path` (only its index and the custom properties
/// part are read).
pub(crate) fn file_document_id(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    package_document_id(std::io::BufReader::new(file))
}

/// The document id in the bytes of a workbook package.
pub(crate) fn bytes_document_id(bytes: &[u8]) -> Option<String> {
    package_document_id(std::io::Cursor::new(bytes))
}

/// This installation's random id: made once and kept in the settings folder. It says which
/// computer saved a version without saying anything about the computer.
pub(crate) fn installation_id(settings: &Path) -> String {
    let path = settings.join(INSTALLATION_FILE);
    if let Ok(meta) = std::fs::symlink_metadata(&path)
        && meta.is_file()
        && meta.len() <= 64
        && let Ok(text) = std::fs::read_to_string(&path)
    {
        let id = text.trim();
        if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return id.to_ascii_lowercase();
        }
    }
    let id = random_bytes::<16>().map(|b| hex(&b)).unwrap_or_default();
    // Not kept (a read-only settings folder): versions of this run still share one id.
    let _ = super::write_private(&path, id.as_bytes());
    id
}

/// The operating system's version number, looked up once per run (never its name for the
/// machine).
pub(crate) fn os_version() -> String {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(read_os_version).clone()
}

fn version_digits(text: &str) -> String {
    let start = text.find(|c: char| c.is_ascii_digit()).unwrap_or(text.len());
    text.get(start..).unwrap_or("").chars().take_while(|c| c.is_ascii_digit() || *c == '.').take(32).collect()
}

fn read_os_version() -> String {
    #[cfg(windows)]
    {
        // `ver` prints "Microsoft Windows [Version 10.0.26100.1]".
        let output = std::process::Command::new("cmd").args(["/d", "/c", "ver"]).output();
        output
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|t| t.rsplit_once(' ').map(|(_, v)| version_digits(v)).unwrap_or_default())
            .unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("sw_vers").arg("-productVersion").output();
        output.ok().and_then(|o| String::from_utf8(o.stdout).ok()).map(|t| version_digits(&t)).unwrap_or_default()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let text = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        text.lines().find_map(|line| line.strip_prefix("VERSION_ID=")).map(|v| version_digits(v.trim_matches('"'))).unwrap_or_default()
    }
}

/// The account name, only so it can be kept out of what is recorded.
fn account_name() -> Option<String> {
    ["USERNAME", "USER", "LOGNAME"].iter().find_map(|k| std::env::var(k).ok()).filter(|n| !n.trim().is_empty())
}

fn home_dir() -> Option<PathBuf> {
    ["USERPROFILE", "HOME"].iter().find_map(std::env::var_os).filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// `path` without Windows' `\\?\` prefix.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    text.strip_prefix(r"\\?\").map_or_else(|| path.to_path_buf(), PathBuf::from)
}

/// `path` below `base`, compared without case on Windows (where file names ignore it).
fn below(path: &Path, base: &Path) -> Option<PathBuf> {
    let (path, base) = (plain(path), plain(base));
    if let Ok(rest) = path.strip_prefix(&base) {
        return Some(rest.to_path_buf());
    }
    if cfg!(windows) {
        let mut parts = path.components();
        for want in base.components() {
            let got = parts.next()?;
            if got.as_os_str().to_string_lossy().to_lowercase() != want.as_os_str().to_string_lossy().to_lowercase() {
                return None;
            }
        }
        return Some(parts.as_path().to_path_buf());
    }
    None
}

fn slashed(rest: &Path) -> String {
    rest.components()
        .filter_map(|c| if let Component::Normal(n) = c { Some(n.to_string_lossy().into_owned()) } else { None })
        .collect::<Vec<_>>()
        .join("/")
}

/// The provider a document is kept with: `local`, `icloud`, `onedrive` or `gdrive`.
pub(crate) fn provider_of(path: &Path, roots: &[gridcraft_cloud::CloudRoot]) -> &'static str {
    gridcraft_cloud::locate(path, roots).map_or("local", |l| l.provider.id())
}

/// The path a version records: below the provider's folder (`OneDrive:/Reports/x.xlsx`) or the
/// user's folder (`~/Documents/x.xlsx`), and any folder named like the account replaced by
/// `<user>`, so the account name is never kept.
pub(crate) fn portable_path(path: &Path, roots: &[gridcraft_cloud::CloudRoot]) -> String {
    let path = plain(path);
    let text = if let Some(location) = gridcraft_cloud::locate(&path, roots)
        && let Some(rest) = below(&path, &location.root)
    {
        format!("{}:/{}", location.provider.id(), slashed(&rest))
    } else if let Some(home) = home_dir()
        && let Some(rest) = below(&path, &home)
    {
        format!("~/{}", slashed(&rest))
    } else {
        path.to_string_lossy().replace('\\', "/")
    };
    match account_name() {
        Some(account) => text
            .split('/')
            .map(|part| if part.eq_ignore_ascii_case(&account) || part.eq_ignore_ascii_case(&format!("{account}.local")) { "<user>" } else { part })
            .collect::<Vec<_>>()
            .join("/"),
        None => text,
    }
}

/// A recorded path back on this computer, when it can be (not when a part was hidden).
pub(crate) fn resolve_portable_path(text: &str, roots: &[gridcraft_cloud::CloudRoot]) -> Option<PathBuf> {
    if text.split('/').any(|part| part == "<user>" || part == "..") {
        return None;
    }
    if let Some(rest) = text.strip_prefix("~/") {
        return home_dir().map(|home| home.join(rest));
    }
    if let Some((provider, rest)) = text.split_once(":/")
        && let Some(provider) = gridcraft_cloud::Provider::from_id(provider)
    {
        return roots.iter().find(|r| r.provider == provider).map(|r| r.path.join(rest));
    }
    let path = PathBuf::from(text);
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_ids_are_random_uuids() {
        let a = new_document_id().unwrap();
        let b = new_document_id().unwrap();
        assert_ne!(a, b);
        assert!(document_id_ok(&a), "{a}");
        assert_eq!(a.as_bytes()[14], b'4', "version 4");
        assert!(matches!(a.as_bytes()[19], b'8' | b'9' | b'a' | b'b'), "RFC variant");
        assert_eq!(id_bucket(&a).len(), 32);
        for bad in [
            "",
            "../../etc/passwd",
            "ZZZZZZZZ-ZZZZ-4ZZZ-8ZZZ-ZZZZZZZZZZZZ",
            "12345678-1234-4234-8234-1234567890AB",
            "12345678123441234823412345678901ab",
        ] {
            assert!(!document_id_ok(bad), "{bad}");
        }
    }

    #[test]
    fn recorded_paths_leave_out_the_account() {
        let home = home_dir().unwrap();
        let file = home.join("Documents").join("Report.xlsx");
        assert_eq!(portable_path(&file, &[]), "~/Documents/Report.xlsx");
        assert_eq!(resolve_portable_path("~/Documents/Report.xlsx", &[]), Some(home.join("Documents/Report.xlsx")));
        let root = gridcraft_cloud::CloudRoot { provider: gridcraft_cloud::Provider::OneDrive, path: home.join("OneDrive"), account: None };
        let cloud = home.join("OneDrive").join("Team").join("Plan.xlsx");
        assert_eq!(portable_path(&cloud, std::slice::from_ref(&root)), "onedrive:/Team/Plan.xlsx");
        assert_eq!(
            resolve_portable_path("onedrive:/Team/Plan.xlsx", std::slice::from_ref(&root)),
            Some(home.join("OneDrive").join("Team/Plan.xlsx"))
        );
        if let Some(account) = account_name() {
            let elsewhere = PathBuf::from(if cfg!(windows) { r"D:\Shared" } else { "/srv/shared" }).join(&account).join("x.xlsx");
            let text = portable_path(&elsewhere, &[]);
            assert!(!text.to_lowercase().contains(&account.to_lowercase()), "{text}");
            assert!(resolve_portable_path(&text, &[]).is_none());
        }
    }
}
