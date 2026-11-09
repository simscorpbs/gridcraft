# AutoSave in synced cloud folders

GridCraft uses the desktop sync clients' ordinary folders. It does not connect to a cloud API. A workbook saved in one of these folders is uploaded by the installed client and becomes available on the person's other devices. Ported from the behaviour of the WordCraft fork (`docs/cloud.md` there); version history is not part of GridCraft yet.

## Folder discovery

On macOS, GridCraft checks `~/Library/Mobile Documents/com~apple~CloudDocs` for iCloud Drive, each `GoogleDrive-<account>` and `OneDrive-<account>` under `~/Library/CloudStorage`, then older `~/Google Drive` and `~/OneDrive` folders. On Windows, iCloud Drive is `%USERPROFILE%\iCloudDrive`; Google Drive may be a drive letter whose root contains `My Drive` (including localized client names) or a mirrored profile folder; OneDrive uses `%OneDriveConsumer%`, `%OneDriveCommercial%` or `%OneDrive%`. On Linux and other systems there is no assumed client path; `GRIDCRAFT_CLOUD_ICLOUD`, `GRIDCRAFT_CLOUD_GDRIVE` and `GRIDCRAFT_CLOUD_ONEDRIVE` can name folders. The browser build has no folders to discover.

The desktop app discovers the roots once, off the UI thread. `file.cloud` reports provider, root, current path, conflict state and the discovered roots. Provider ids are `icloud`, `gdrive` and `onedrive`. iCloud-only files must be downloaded by the sync client before opening.

## Save a new workbook

Turning AutoSave on for a workbook that was never saved opens the Save to Cloud dialog (or Save As when no cloud folder was found). `file.saveToCloud` accepts optional `provider` and `name` (without extension). With no provider, the first discovered root is chosen in iCloud Drive, Google Drive, OneDrive order. The file is written as a new `.xlsx` in that provider's documents folder, using a numbered name when needed.

## Conflict protection

`file.save` with `{"ifUnchanged": true}` (what AutoSave sends) first checks that the workbook's file still holds what GridCraft last read or wrote (size, date and a digest of the bytes). If another device or program changed it, or it was deleted or moved, nothing is written: the result has `conflict` (`changed` or `missing`), the Save Conflict dialog opens and AutoSave stays paused for that workbook. `file.resolveConflict` takes `keep`: `both` (save this version beside the other as `Name (conflict copy).xlsx` and keep editing that), `mine` (overwrite; for a missing file, save it here again) or `theirs` (reopen the file as it is now, discarding unsaved changes here). A plain Save is the person's own decision and still writes.

## Closing and quitting

With AutoSave on, closing a changed workbook (`file.close`, `window.close`, quitting) first writes the pending changes with `ifUnchanged` and goes on without a Save Changes prompt. If another device changed the file, the Save Conflict dialog opens instead and the workbook stays open. If a Save Changes prompt was already visible, a successful AutoSave closes it and the workbook. With AutoSave off for the file, or a write that fails, the Save Changes prompt stays in charge.

## AutoSave for each file

AutoSave is on by default for every workbook whose file exists on disk in a format that keeps everything (`.xlsx`, `.xlsm`, `.xltx`, `.xltm` and GridCraft's own `.json`), local or in a cloud folder, and an opened file is covered at once. The switch is off, with the reason in its tooltip, for a new workbook, a format that loses things (csv, text, web page), a workbook opened from bytes, the browser, and a file with a conflict waiting.

A write that fails (another program holds the file, a full disk) does not turn AutoSave off. It pauses and tries again after 5 seconds, then waits twice as long after each further failure, up to one minute. The status bar shows the error while writes keep failing and the message goes away after the next successful write. Successful writes are at least two seconds apart.

Each file remembers its switch in `<settings_dir>/autosave.json`, never in the workbook. Turning AutoSave off for a file is remembered when it is opened again; turning it back on forgets the choice. Paths are normalised (on Windows the `\?\` prefix is removed, separators become `\` and case is ignored; case is also ignored on macOS). The file keeps at most 2000 entries, drops the oldest first, is read only up to 1 MiB and is written atomically. Save As, Save to Cloud and Keep both give the new file the switch the workbook had.

File › Options › Save › **Turn on AutoSave by default** (`file.options` with `{"autosaveByDefault": bool}`, saved with the preferences as `autosaveByDefault`) sets the default for files with no choice of their own. It is on by default.

`file.autosave` with `{"value": bool}` (or `{}` to toggle) sets the active workbook's switch and remembers it for the file; it fails with the reason when AutoSave cannot cover the workbook. `file.cloud` reports `autosave: {on, default, remembered, covers, blocked}`.

## Limits

The feature depends on a compatible sync client being installed and signed in. There is no cloud API, server-side merge, real-time co-authoring or version history. Renaming files is not a GridCraft command, so the remembered choice does not need to follow a rename.
