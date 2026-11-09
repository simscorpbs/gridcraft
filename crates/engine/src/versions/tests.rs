//! The version store: cadence, retention, identity-free storage of package parts, mirrors and
//! synced folders, damage and repair. The save-path tests are in `crate::tests::versions`.

use super::*;
use serde_json::json;
use std::io::Write;

#[test]
fn canonical_key_is_stable_and_bounded() {
    let path = Path::new("relative/notes.xlsx");
    assert_eq!(document_key(path), document_key(path));
    assert_eq!(document_key(path).len(), 16);
}

#[test]
fn iso_parser_accepts_engine_timestamps() {
    let now = 1_800_000_000;
    let value = clock::iso_from_unix_secs(now);
    assert_eq!(parse_iso_secs(&value), Some(now));
}

pub(super) fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("gridcraft-versions-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn put(root: &Path, file: &Path, bytes: &[u8], kind: VersionKind, when: u64, now: u64, session: Option<&str>) -> Vec<VersionInfo> {
    store_save_at(
        &[root.to_path_buf()],
        VersionWrite {
            path: file.to_path_buf(),
            bytes: bytes.to_vec(),
            author: "Tester".into(),
            kind,
            saved_at: clock::iso_from_unix_secs(when),
            session: session.map(str::to_owned),
            document_id: None,
            details: VersionDetails { trigger: "save".into(), ..VersionDetails::default() },
            cloud: Vec::new(),
        },
        now,
    )
    .unwrap()
    .0
}

fn write(root: &Path, file: &Path, text: &str, kind: VersionKind, when: u64, now: u64) -> Vec<VersionInfo> {
    put(root, file, text.as_bytes(), kind, when, now, Some("run-1"))
}

pub(super) fn save_bytes_at(root: &Path, file: &Path, bytes: &[u8], when: u64) -> Vec<VersionInfo> {
    put(root, file, bytes, VersionKind::Save, when, when, None)
}

/// Two zip packages with the same parts: names, order, methods, dates and contents.
pub(super) fn same_parts(a: &[u8], b: &[u8]) -> bool {
    let parts = |bytes: &[u8]| pack::split_zip(bytes).map(|(parts, comment)| (parts.into_iter().map(|p| p.meta).collect::<Vec<_>>(), comment));
    parts(a).is_some() && parts(a) == parts(b)
}

pub(super) fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// The sheet of a workbook with `paragraphs` rows of text, the `edit`-th one changed.
pub(super) fn body_xml(paragraphs: usize, edit: usize) -> Vec<u8> {
    let mut xml = String::from("<?xml version=\"1.0\"?><worksheet><sheetData>");
    for i in 0..paragraphs {
        let words = if i == edit % paragraphs.max(1) {
            format!("Paragraph {i} as revised in edit {edit}, with a new sentence.")
        } else {
            format!("Paragraph {i} of the report says what it says.")
        };
        xml.push_str(&format!("<row r=\"{}\"><c r=\"A{}\" t=\"inlineStr\"><is><t>{words}</t></is></c></row>", i + 1, i + 1));
    }
    xml.push_str("</sheetData></worksheet>");
    xml.into_bytes()
}

/// A sheet of varied prose (it compresses like real text, not like a repeated line).
fn varied_body_xml(paragraphs: usize, edit: usize) -> Vec<u8> {
    const WORDS: &[&str] = &[
        "the",
        "court",
        "found",
        "that",
        "contract",
        "was",
        "signed",
        "after",
        "notice",
        "parties",
        "agreed",
        "payment",
        "within",
        "thirty",
        "days",
        "account",
        "access",
        "restored",
        "evidence",
        "shows",
        "request",
        "denied",
        "claim",
        "damages",
        "moral",
        "platform",
        "user",
        "terms",
        "breach",
        "hearing",
        "judge",
        "appeal",
        "record",
        "filed",
        "deadline",
        "expert",
        "report",
        "witness",
        "statement",
        "judgment",
    ];
    let mut xml = String::from("<?xml version=\"1.0\"?><worksheet><sheetData>");
    for i in 0..paragraphs {
        let seed = if i == edit % paragraphs.max(1) { (i as u64) << 20 | edit as u64 } else { i as u64 };
        let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        let mut words = Vec::new();
        for _ in 0..(12 + (seed % 30)) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            words.push(WORDS[(state % WORDS.len() as u64) as usize]);
        }
        xml.push_str(&format!("<row r=\"{}\"><c r=\"A{}\" t=\"inlineStr\"><is><t>{}.</t></is></c></row>", i + 1, i + 1, words.join(" ")));
    }
    xml.push_str("</sheetData></worksheet>");
    xml.into_bytes()
}

/// A `.xlsx`-shaped package: a deflated body and styles and a stored picture, like Excel's.
pub(super) fn package(body: &[u8], picture: &[u8], extra: Option<(&str, &[u8])>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut part = |name: &str, data: &[u8], options| {
        writer.start_file(name, options).unwrap();
        writer.write_all(data).unwrap();
    };
    part("[Content_Types].xml", b"<Types><Default Extension=\"xml\"/><Default Extension=\"png\"/></Types>", deflated);
    part("xl/worksheets/sheet1.xml", body, deflated);
    part("xl/styles.xml", "<styleSheet>".repeat(2_000).as_bytes(), deflated);
    part("xl/media/image1.png", picture, stored);
    if let Some((name, data)) = extra {
        part(name, data, deflated);
    }
    writer.finish().unwrap().into_inner()
}

fn read_at(root: &Path, file: &Path, id: &str) -> Result<Vec<u8>, String> {
    let dir = document_dir(root, file);
    let dirs = [dir.clone()];
    match read_index(&dir).into_iter().find(|v| v.id == id) {
        Some(info) if info.storage == VersionStorage::Parts => read_parts_version(&dirs, id),
        Some(info) => read_whole_version(&dirs, &info),
        None => Err("no such version".into()),
    }
}

fn objects_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join(OBJECTS_DIR))
        .map(|entries| entries.flatten().filter_map(|e| e.file_name().to_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    names.sort();
    names
}

fn disk_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let meta = entry.metadata().unwrap();
        total += if meta.is_dir() { disk_bytes(&entry.path()) } else { meta.len() };
    }
    total
}

#[test]
fn autosaves_join_within_ten_minutes_but_not_after() {
    let root = temp("join");
    let file = root.join("note.json");
    let first = write(&root, &file, "one", VersionKind::Autosave, 1_800_000_000, 1_800_000_000);
    let second = write(&root, &file, "two", VersionKind::Autosave, 1_800_000_599, 1_800_000_599);
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].size, 3);
    assert_eq!(second[0].supersedes.as_deref(), Some(first[0].id.as_str()), "a join writes a new record, never rewrites one");
    // The window is counted from its first write, not the latest: continuous editing still
    // leaves a version every ten minutes.
    let third = write(&root, &file, "three", VersionKind::Autosave, 1_800_000_601, 1_800_000_601);
    assert_eq!(third.len(), 2, "ten minutes after the window opened, a new version starts");
    let fourth = write(&root, &file, "four", VersionKind::Autosave, 1_800_001_200, 1_800_001_200);
    assert_eq!(fourth.len(), 2, "joins the window that opened at the third write");
    assert_eq!(fourth[0].size, 4);
    let fifth = write(&root, &file, "five", VersionKind::Autosave, 1_800_000_601 + AUTOSAVE_WINDOW_SECS, 1_800_000_601 + AUTOSAVE_WINDOW_SECS);
    assert_eq!(fifth.len(), 3);
    // An explicit save is always its own version.
    let saved = write(&root, &file, "saved", VersionKind::Save, 1_800_001_202, 1_800_001_202);
    assert_eq!(saved.len(), 4);
    // Superseded records are gone from the folder too.
    let dir = document_dir(&root, &file);
    assert_eq!(read_index(&dir).len(), 4);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn first_autosave_after_reopening_starts_a_new_version() {
    let root = temp("reopen");
    let file = root.join("note.json");
    let now = 1_800_000_000;
    assert_eq!(put(&root, &file, b"first run", VersionKind::Autosave, now, now, Some("run-1")).len(), 1);
    let list = put(&root, &file, b"second run", VersionKind::Autosave, now + 120, now + 120, Some("run-2"));
    assert_eq!(list.len(), 2, "a new editing session never overwrites the previous session's version");
    assert_eq!(list[1].size, "first run".len() as u64);
    assert_eq!(put(&root, &file, b"second run, more", VersionKind::Autosave, now + 180, now + 180, Some("run-2")).len(), 2);
    assert_eq!(put(&root, &file, b"kept", VersionKind::Autosave, now + 200, now + 200, None).len(), 3);
    assert_eq!(put(&root, &file, b"after", VersionKind::Autosave, now + 210, now + 210, Some("run-2")).len(), 4);
    let _ = fs::remove_dir_all(root);
}

fn entry(n: usize, at: u64, kind: VersionKind) -> VersionInfo {
    VersionInfo { id: format!("{n:032x}"), saved_at: clock::iso_from_unix_secs(at), author: "Tester".into(), size: 1, kind, ..VersionInfo::default() }
}

#[test]
fn pruning_follows_the_automatic_schedule() {
    let root = temp("prune");
    let day = DAY_SECS;
    let now: u64 = 1_800_000_000 / day * day;
    let mut entries = Vec::new();
    let mut n = 0usize;
    let mut add = |entries: &mut Vec<VersionInfo>, at: u64| {
        n += 1;
        entries.push(entry(n, at, VersionKind::Save));
        n
    };
    let recent: Vec<usize> = (0..5).map(|i| add(&mut entries, now - 29 * day + i)).collect();
    let hour_a_old = add(&mut entries, now - 40 * day + 60);
    let hour_a_new = add(&mut entries, now - 40 * day + 120);
    let hour_b = add(&mut entries, now - 40 * day + 3_600 + 60);
    let day_old = add(&mut entries, now - 100 * day + 3_600);
    let day_new = add(&mut entries, now - 100 * day + 7_200);
    let next_day = add(&mut entries, now - 99 * day + 3_600);
    let week = (((now - 400 * day) / day + 3) / 7 * 7 - 3) * day;
    let week_old = add(&mut entries, week + 3_600);
    let week_new = add(&mut entries, week + day);
    let ancient = add(&mut entries, now - 3_000 * day);
    let (kept, removed) = prune(&root, entries, now);
    let kept: BTreeSet<String> = kept.into_iter().map(|v| v.id).collect();
    let has = |n: usize| kept.contains(&format!("{n:032x}"));
    assert!(recent.iter().all(|&n| has(n)), "every version of the last 30 days stays");
    assert!(has(hour_a_new) && !has(hour_a_old) && has(hour_b), "one per hour between 30 and 60 days");
    assert!(has(day_new) && !has(day_old) && has(next_day), "one per day between 60 and 180 days");
    assert!(has(week_new) && !has(week_old), "one per week after 180 days");
    assert!(has(ancient), "a document left alone keeps its old history");
    assert!(removed.contains(&format!("{hour_a_old:032x}")));
    let many: Vec<VersionInfo> = (0..MAX_VERSIONS + 20).map(|i| entry(i + 1, now - u64::try_from(i).unwrap(), VersionKind::Save)).collect();
    let (kept, _) = prune(&root, many, now);
    assert_eq!(kept.len(), MAX_VERSIONS);
    let large: Vec<VersionInfo> =
        (0..100).map(|i| VersionInfo { size: 6 << 20, ..entry(i + 1, now - u64::try_from(i).unwrap(), VersionKind::Save) }).collect();
    let (kept, _) = prune(&root, large, now);
    assert!(kept.iter().map(|v| v.size).sum::<u64>() <= MAX_DOCUMENT_BYTES);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_first_original_is_pinned_past_every_limit() {
    let root = temp("pinned");
    let now: u64 = 1_800_000_000;
    let mut list = vec![VersionInfo { size: 10 << 20, ..entry(1, now - 5_000 * DAY_SECS, VersionKind::Original) }];
    list.extend((0..MAX_VERSIONS + 10).map(|i| VersionInfo { size: 1 << 20, ..entry(i + 2, now - u64::try_from(i).unwrap(), VersionKind::Save) }));
    let (kept, _) = prune(&root, list, now);
    assert!(kept.len() <= MAX_VERSIONS);
    assert_eq!(kept.last().map(|v| v.kind), Some(VersionKind::Original));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn records_from_earlier_builds_migrate_and_damaged_files_cost_nothing() {
    let root = temp("legacy-index");
    let file = root.join("note.json");
    let dir = document_dir(&root, &file);
    fs::create_dir_all(&dir).unwrap();
    // An earlier build's folder: `.bin` files and one shared index.json.
    let ids = ["0123456789abcdef0123456789abcdef", "fedcba9876543210fedcba9876543210"];
    let mut legacy = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        fs::write(dir.join(format!("{id}.bin")), format!("old {i}")).unwrap();
        legacy.push(json!({"id": id, "savedAt": clock::iso_from_unix_secs(1_800_000_000 + i as u64), "author": "Tester", "size": 5, "kind": "save"}));
    }
    legacy.push(json!({"id": 7, "kind": "future"}));
    fs::write(dir.join(INDEX_FILE), serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(read_index(&dir).len(), 2, "read as it is, one bad entry costing only itself");
    let list = write(&root, &file, "new", VersionKind::Save, 1_800_000_100, 1_800_000_100);
    assert_eq!(list.len(), 3);
    assert!(!dir.join(INDEX_FILE).exists(), "the shared index is gone once each version has its own record");
    assert!(ids.iter().all(|id| dir.join(format!("{id}{META_SUFFIX}")).is_file()));
    // A damaged record or a version file nothing describes never costs the bytes.
    fs::write(dir.join(format!("{}{META_SUFFIX}", ids[0])), b"{ damaged").unwrap();
    let recovered = read_index(&dir);
    assert_eq!(recovered.len(), 3);
    assert!(dir.join(format!("{}.bin", ids[0])).is_file());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_sync_apps_conflict_copies_are_ignored_and_left_alone() {
    let root = temp("conflicts");
    let file = root.join("Shared.xlsx");
    let picture = noise(2_000, 3);
    let list = save_bytes_at(&root, &file, &package(&body_xml(50, 0), &picture, None), 1_800_000_000);
    let dir = document_dir(&root, &file);
    let id = list[0].id.clone();
    // What OneDrive, Dropbox and iCloud leave beside a file both computers changed.
    let strays = [
        format!("{id} (1){PARTS_SUFFIX}"),
        format!("{id}-DESKTOP-4F2K9{PARTS_SUFFIX}"),
        format!("{id} (conflicted copy){META_SUFFIX}"),
        "index (1).json".to_owned(),
        "document-LAPTOP.json".to_owned(),
    ];
    for stray in &strays {
        fs::copy(manifest_path(&dir, &id), dir.join(stray)).unwrap();
    }
    let object = objects_in(&dir)[0].clone();
    fs::copy(object_path(&dir, &object), dir.join(OBJECTS_DIR).join(format!("{object} (1)"))).unwrap();
    assert_eq!(visible(&read_index(&dir)).len(), 1);
    let list = save_bytes_at(&root, &file, &package(&body_xml(50, 1), &picture, None), 1_800_000_060);
    assert_eq!(list.len(), 2);
    assert!(strays.iter().all(|stray| dir.join(stray).is_file()), "never deleted on a guess");
    assert!(dir.join(OBJECTS_DIR).join(format!("{object} (1)")).is_file());
    assert!(same_parts(&read_at(&root, &file, &id).unwrap(), &package(&body_xml(50, 0), &picture, None)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn damaged_version_bytes_are_refused() {
    let root = temp("checksum");
    let file = root.join("note.json");
    let list = write(&root, &file, "good", VersionKind::Save, 1_800_000_000, 1_800_000_000);
    let id = list[0].id.clone();
    fs::write(document_dir(&root, &file).join(format!("{id}.bin")), b"evil").unwrap();
    let store = Versions::start(root.clone(), Vec::new(), std::sync::Arc::new(|| {})).unwrap();
    assert!(store.read(&file, None, &id).is_err(), "same size, different bytes");
    drop(store);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_mirror_that_is_away_is_brought_up_to_date_when_it_returns() {
    let root = temp("mirror");
    let local = root.join("local");
    let away = root.join("usb");
    fs::write(&away, b"the drive is not there").unwrap();
    let file = root.join("Mirror.xlsx");
    let picture = noise(2_000, 5);
    let roots = vec![local.clone(), away.clone()];
    let save = |edit: usize, when: u64| {
        store_save_at(
            &roots,
            VersionWrite {
                path: file.clone(),
                bytes: package(&body_xml(80, edit), &picture, None),
                author: "Tester".into(),
                kind: VersionKind::Save,
                saved_at: clock::iso_from_unix_secs(when),
                session: None,
                document_id: None,
                details: VersionDetails::default(),
                cloud: Vec::new(),
            },
            when,
        )
    };
    let (list, unavailable) = save(0, 1_800_000_000).unwrap();
    assert_eq!(list.len(), 1, "the save goes on without the mirror");
    assert_eq!(unavailable, vec![away.clone()]);
    // The drive comes back.
    fs::remove_file(&away).unwrap();
    fs::create_dir_all(&away).unwrap();
    let (list, unavailable) = save(1, 1_800_000_060).unwrap();
    assert!(unavailable.is_empty());
    let (a, b) = (document_dir(&local, &file), document_dir(&away, &file));
    assert_eq!(visible(&read_index(&b)).len(), 2, "the version it missed was copied over");
    assert_eq!(objects_in(&a), objects_in(&b));
    // Either folder alone gives every version back.
    fs::remove_dir_all(&a).unwrap();
    for v in &list {
        assert!(read_parts_version(&[a.clone(), b.clone()], &v.id).is_ok());
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn two_computers_writing_to_one_synced_folder_never_undo_each_other() {
    let root = temp("two-computers");
    let shared = root.join("OneDrive-history");
    let file = root.join("Joint.xlsx");
    let picture = noise(3_000, 9);
    let id = meta::new_document_id().unwrap();
    let write_from = |device: &str, edit: usize, when: u64| {
        store_save_at(
            std::slice::from_ref(&shared),
            VersionWrite {
                path: file.clone(),
                bytes: package(&body_xml(60, edit), &picture, None),
                author: device.into(),
                kind: VersionKind::Autosave,
                saved_at: clock::iso_from_unix_secs(when),
                session: Some(device.into()),
                document_id: Some(id.clone()),
                details: VersionDetails { device: device.into(), ..VersionDetails::default() },
                cloud: Vec::new(),
            },
            when,
        )
        .unwrap()
        .0
    };
    let now = 1_800_000_000;
    write_from("Windows", 0, now);
    write_from("Mac", 1, now + 30);
    write_from("Windows", 2, now + 60);
    let list = write_from("Mac", 3, now + 90);
    assert_eq!(list.len(), 4, "each computer's session keeps its own versions");
    let dir = shared.join(meta::id_bucket(&id));
    assert!(!dir.join(INDEX_FILE).exists(), "no shared index to fight over");
    // An object that arrived before its manifest (the sync app is behind) is not deleted.
    let late = noise(4_000, 77);
    let late_hash = digest_hex(&late);
    fs::write(object_path(&dir, &late_hash), pack::encode_full(&late)).unwrap();
    let kept = visible(&read_index(&dir));
    collect_objects(&dir, &kept, now + 100);
    assert!(object_path(&dir, &late_hash).is_file(), "marked, not deleted");
    collect_objects(&dir, &kept, now + 100 + 6 * DAY_SECS);
    assert!(object_path(&dir, &late_hash).is_file(), "still inside the grace period");
    // Its manifest arrives: the mark goes and the object stays for good.
    let mut info = entry(99, now + 3 * DAY_SECS, VersionKind::Save);
    info.size = 1;
    let manifest = pack::Manifest {
        format: 1,
        info: VersionInfo { storage: VersionStorage::Parts, ..info.clone() },
        comment: String::new(),
        parts: vec![pack::ManifestPart {
            name: "late.bin".into(),
            method: pack::Method::Stored,
            date: 0,
            time: 0,
            mode: None,
            dir: false,
            size: late.len() as u64,
            sha256: late_hash.clone(),
        }],
    };
    fs::write(manifest_path(&dir, &info.id), serde_json::to_vec(&manifest).unwrap()).unwrap();
    let kept = visible(&read_index(&dir));
    collect_objects(&dir, &kept, now + 100 + 8 * DAY_SECS);
    assert!(object_path(&dir, &late_hash).is_file());
    // Without one, it goes after the grace period.
    fs::remove_file(manifest_path(&dir, &info.id)).unwrap();
    let kept = visible(&read_index(&dir));
    collect_objects(&dir, &kept, now + 100 + 9 * DAY_SECS);
    collect_objects(&dir, &kept, now + 100 + 17 * DAY_SECS);
    assert!(!object_path(&dir, &late_hash).exists());
    let _ = fs::remove_dir_all(root);
}

#[cfg(windows)]
#[test]
fn windows_keys_ignore_case_and_older_folders_move_over() {
    let root = temp("case");
    let file = root.join("Case.json");
    fs::write(&file, b"x").unwrap();
    let upper = root.join("CASE.JSON");
    assert_eq!(document_key(&file), document_key(&upper));
    let versions = root.join("versions");
    let legacy = versions.join(legacy_key(&file));
    assert_ne!(legacy, versions.join(document_key(&file)));
    let list = write(&versions, &file, "kept", VersionKind::Save, 1_800_000_000, 1_800_000_000);
    fs::rename(document_dir(&versions, &file), &legacy).unwrap();
    assert_eq!(
        visible(&read_index(&document_dir(&versions, &upper))).iter().map(|v| v.id.clone()).collect::<Vec<_>>(),
        list.iter().map(|v| v.id.clone()).collect::<Vec<_>>()
    );
    assert!(!legacy.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_picture_is_stored_once_and_edits_are_small_deltas() {
    let root = temp("dedupe");
    let file = root.join("Report.xlsx");
    let picture = noise(300_000, 7);
    let now = 1_800_000_000;
    let first = package(&body_xml(400, 0), &picture, None);
    let list = save_bytes_at(&root, &file, &first, now);
    assert_eq!(list[0].storage, VersionStorage::Parts);
    let first_stored = list[0].stored.unwrap();
    let second = package(&body_xml(400, 1), &picture, None);
    let list = save_bytes_at(&root, &file, &second, now + 60);
    let second_stored = list[0].stored.unwrap();
    assert!(first_stored > picture.len() as u64);
    assert!(second_stored < 5_000, "an edit costs a small delta, not the picture again: {second_stored} bytes");
    let dir = document_dir(&root, &file);
    assert_eq!(object_header(&dir, &digest_hex(&picture)).unwrap().0.depth, 0);
    assert!(same_parts(&read_at(&root, &file, &list[1].id).unwrap(), &first));
    assert!(same_parts(&read_at(&root, &file, &list[0].id).unwrap(), &second));
    assert_eq!(list[0].size, second.len() as u64);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn changed_parts_chain_deltas_and_start_again_whole() {
    let root = temp("chain");
    let file = root.join("Chain.xlsx");
    let picture = noise(1_000, 9);
    let now = 1_800_000_000;
    let count = usize::from(pack::KEYFRAME_INTERVAL) + 3;
    let packages: Vec<Vec<u8>> = (0..count).map(|edit| package(&body_xml(200, edit), &picture, None)).collect();
    for (i, bytes) in packages.iter().enumerate() {
        let _ = save_bytes_at(&root, &file, bytes, now + i as u64);
    }
    let dir = document_dir(&root, &file);
    let depths: Vec<u16> = packages
        .iter()
        .map(|bytes| {
            let (parts, _) = pack::split_zip(bytes).unwrap();
            let body = parts.iter().find(|p| p.meta.name == "xl/worksheets/sheet1.xml").unwrap();
            object_header(&dir, &body.meta.sha256).unwrap().0.depth
        })
        .collect();
    let interval = pack::KEYFRAME_INTERVAL;
    let mut expected: Vec<u16> = (0..=interval).collect();
    expected.extend(0..u16::try_from(count).unwrap() - interval - 1);
    assert_eq!(depths, expected);
    let list = visible(&read_index(&dir));
    for (entry, bytes) in list.iter().rev().zip(&packages) {
        assert!(same_parts(&read_at(&root, &file, &entry.id).unwrap(), bytes));
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_damaged_part_only_affects_the_versions_that_need_it() {
    let root = temp("damaged-part");
    let file = root.join("Fragile.xlsx");
    let picture = noise(2_000, 11);
    let now = 1_800_000_000;
    let packages: Vec<Vec<u8>> = (0..4).map(|edit| package(&body_xml(150, edit), &picture, None)).collect();
    for (i, bytes) in packages.iter().enumerate() {
        let _ = save_bytes_at(&root, &file, bytes, now + i as u64);
    }
    let dir = document_dir(&root, &file);
    let list = visible(&read_index(&dir));
    let body_of =
        |bytes: &[u8]| pack::split_zip(bytes).unwrap().0.into_iter().find(|p| p.meta.name == "xl/worksheets/sheet1.xml").unwrap().meta.sha256;
    let third = object_path(&dir, &body_of(&packages[2]));
    let mut bytes = fs::read(&third).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x55;
    fs::write(&third, bytes).unwrap();
    assert!(same_parts(&read_at(&root, &file, &list[3].id).unwrap(), &packages[0]));
    assert!(same_parts(&read_at(&root, &file, &list[2].id).unwrap(), &packages[1]));
    assert!(read_at(&root, &file, &list[1].id).is_err());
    assert!(read_at(&root, &file, &list[0].id).is_err());
    fs::remove_file(object_path(&dir, &digest_hex(&picture))).unwrap();
    assert!(read_at(&root, &file, &list[3].id).is_err());
    let _ = save_bytes_at(&root, &file, &packages[2], now + 10);
    assert!(same_parts(&read_at(&root, &file, &list[1].id).unwrap(), &packages[2]));
    assert!(same_parts(&read_at(&root, &file, &list[0].id).unwrap(), &packages[3]));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unused_parts_are_collected_after_the_grace_period_but_delta_bases_stay() {
    let root = temp("gc");
    let file = root.join("Collect.xlsx");
    let picture = noise(5_000, 13);
    let only_first = noise(4_000, 14);
    let now = 1_800_000_000;
    let first = package(&body_xml(150, 0), &picture, Some(("xl/media/old.bin", &only_first)));
    let list = save_bytes_at(&root, &file, &first, now);
    let first_id = list[0].id.clone();
    let second = package(&body_xml(150, 1), &picture, None);
    let list = save_bytes_at(&root, &file, &second, now + 1);
    let dir = document_dir(&root, &file);
    let body_first = pack::split_zip(&first).unwrap().0.into_iter().find(|p| p.meta.name == "xl/worksheets/sheet1.xml").unwrap().meta.sha256;
    let kept: Vec<VersionInfo> = list.into_iter().filter(|v| v.id != first_id).collect();
    remove_versions(&dir, &[first_id.clone()].into_iter().collect());
    collect_objects(&dir, &kept, now + 2);
    assert!(objects_in(&dir).contains(&digest_hex(&only_first)), "marked, kept for the grace period");
    collect_objects(&dir, &kept, now + 2 + ORPHAN_GRACE_SECS);
    let objects = objects_in(&dir);
    assert!(!objects.contains(&digest_hex(&only_first)), "a part only the removed version had is collected");
    assert!(objects.contains(&body_first), "the base of a delta still in use stays");
    assert!(same_parts(&read_at(&root, &file, &kept[0].id).unwrap(), &second));
    fs::write(manifest_path(&dir, &kept[0].id), b"{ damaged").unwrap();
    let before = objects_in(&dir);
    collect_objects(&dir, &kept, now + 3 + 2 * ORPHAN_GRACE_SECS);
    assert_eq!(objects_in(&dir), before, "an unreadable manifest stops collection");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn whole_versions_from_earlier_builds_stay_readable_and_move_to_parts() {
    let root = temp("migrate");
    let file = root.join("Old.xlsx");
    let picture = noise(20_000, 15);
    let now = 1_800_000_000;
    let old = package(&body_xml(100, 0), &picture, None);
    let dir = document_dir(&root, &file);
    fs::create_dir_all(&dir).unwrap();
    let id = "0123456789abcdef0123456789abcdef";
    fs::write(dir.join(format!("{id}.bin")), &old).unwrap();
    let legacy = json!([{"id": id, "savedAt": clock::iso_from_unix_secs(now), "author": "Tester", "size": old.len(), "kind": "save", "digest": digest_hex(&old)}]);
    fs::write(dir.join(INDEX_FILE), serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(read_at(&root, &file, id).unwrap(), old);
    let newer = package(&body_xml(100, 1), &picture, None);
    let _ = save_bytes_at(&root, &file, &newer, now + 60);
    let list = visible(&read_index(&dir));
    let migrated = list.iter().find(|v| v.id == id).unwrap();
    assert_eq!(migrated.storage, VersionStorage::Parts, "moved to parts at the next save");
    assert!(!dir.join(format!("{id}.bin")).exists());
    assert!(same_parts(&read_at(&root, &file, id).unwrap(), &old));
    assert_eq!(migrated.digest.as_deref(), Some(digest_hex(&old).as_str()));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_original_and_files_that_are_not_zips_stay_byte_for_byte() {
    let root = temp("verbatim");
    let file = root.join("Original.xlsx");
    let original = package(&body_xml(50, 0), &noise(1_000, 17), None);
    let list = put(&root, &file, &original, VersionKind::Original, 1_800_000_000, 1_800_000_000, None);
    assert_eq!(list[0].storage, VersionStorage::Whole);
    assert_eq!(read_at(&root, &file, &list[0].id).unwrap(), original);
    let broken = original.get(..original.len() / 2).unwrap().to_vec();
    let list = save_bytes_at(&root, &file, &broken, 1_800_000_100);
    assert_eq!(list[0].storage, VersionStorage::Whole);
    assert_eq!(read_at(&root, &file, &list[0].id).unwrap(), broken);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_size_limit_counts_real_disk_space() {
    let root = temp("space");
    let file = root.join("Space.xlsx");
    let dir = document_dir(&root, &file);
    let picture = noise(50_000, 19);
    let now = 1_800_000_000;
    let mut logical = 0u64;
    for edit in 0..20 {
        let bytes = package(&body_xml(100, edit), &picture, None);
        logical += bytes.len() as u64;
        let _ = save_bytes_at(&root, &file, &bytes, now + edit as u64);
    }
    assert_eq!(visible(&read_index(&dir)).len(), 20);
    let real = disk_bytes(&dir);
    assert!(real * 5 < logical, "20 versions take {real} bytes on disk instead of {logical}");
    let _ = fs::remove_dir_all(root);
}

/// The figure for the docs: 100 versions of a ~1.8 MB `.xlsx` with a picture, one paragraph
/// edited each time. Run with `cargo test -p gridcraft-engine --lib storage_figure -- --ignored --nocapture`.
#[test]
#[ignore = "slow; prints the storage figure for docs/cloud.md"]
fn storage_figure_for_a_hundred_versions() {
    let root = temp("figure");
    let file = root.join("Figure.xlsx");
    let dir = document_dir(&root, &file);
    let picture = noise(1_700_000, 21);
    let now = 1_800_000_000;
    let mut whole = 0u64;
    let mut size = 0;
    for edit in 0..100 {
        let bytes = package(&varied_body_xml(3_000, edit * 7), &picture, None);
        size = bytes.len();
        whole += bytes.len() as u64;
        let _ = save_bytes_at(&root, &file, &bytes, now + edit as u64 * 60);
    }
    let parts = disk_bytes(&dir);
    println!("100 versions of a {size}-byte .xlsx: {whole} bytes kept whole, {parts} bytes as parts ({:.1}%)", parts as f64 * 100.0 / whole as f64);
    assert!(parts * 10 < whole);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_zip_bomb_is_stored_whole_and_never_inflated() {
    let root = temp("bomb");
    let file = root.join("Bomb.xlsx");
    // A part that declares more than the 256 MiB a part may have: a few hundred KiB on disk.
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer.start_file("xl/worksheets/sheet1.xml", options).unwrap();
    let zeros = vec![0u8; 1 << 20];
    for _ in 0..=(pack::MAX_PART_BYTES >> 20) {
        writer.write_all(&zeros).unwrap();
    }
    let bomb = writer.finish().unwrap().into_inner();
    assert!(bomb.len() < 4 << 20, "{}", bomb.len());
    assert!(pack::split_zip(&bomb).is_none(), "over the limit: not opened");
    let list = save_bytes_at(&root, &file, &bomb, 1_800_000_000);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].storage, VersionStorage::Whole, "kept as the archive it is");
    assert_eq!(read_at(&root, &file, &list[0].id).unwrap(), bomb);
    // Too many entries count the same way.
    let mut many = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..=pack::MAX_PARTS {
        many.start_file(format!("p{i}"), options).unwrap();
    }
    let many = many.finish().unwrap().into_inner();
    let list = save_bytes_at(&root, &file, &many, 1_800_000_100);
    assert_eq!(list[0].storage, VersionStorage::Whole);
    let _ = fs::remove_dir_all(root);
}
