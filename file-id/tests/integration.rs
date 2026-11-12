use file_id::{get_file_id, get_file_id_from_file, get_file_id_no_follow};
use std::{fs, io, path::Path};
use tempfile::TempDir;

/// Creates a file symlink.
///
/// On Windows, requires Developer Mode or `SeCreateSymbolicLinkPrivilege`
/// (e.g. an elevated shell). Returns an error if symlink creation fails.
fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(target_family = "unix")]
    return std::os::unix::fs::symlink(target, link);

    #[cfg(target_family = "windows")]
    return std::os::windows::fs::symlink_file(target, link).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!(
                "creating a symlink failed ({err}); enable Developer Mode or run the tests elevated"
            ),
        )
    });
}

// Following a symlink returns the target's ID; the link has its own stable ID.
#[test]
fn test_symlink_id_differs_from_target_and_is_stable() -> io::Result<()> {
    let temp_dir = TempDir::new()?;
    let file_path = temp_dir.path().join("test_file.txt");
    let symlink_path = temp_dir.path().join("test_symlink");

    fs::write(&file_path, "test content")?;
    symlink_file(&file_path, &symlink_path)?;

    let original_file_id = get_file_id(&file_path)?;
    let symlink_follow_id = get_file_id(&symlink_path)?;
    let symlink_no_follow_id = get_file_id_no_follow(&symlink_path)?;

    // Following the symlink should give us the same ID as the original file
    assert_eq!(original_file_id, symlink_follow_id);

    // Not following the symlink should give us a different ID (the symlink's own ID)
    assert_ne!(original_file_id, symlink_no_follow_id);

    // The symlink's own ID is stable
    assert_eq!(symlink_no_follow_id, get_file_id_no_follow(&symlink_path)?);

    Ok(())
}

// All three lookup methods return the same ID for a regular file.
#[test]
fn test_regular_file_ids_match() -> io::Result<()> {
    let temp_dir = TempDir::new()?;
    let file_path = temp_dir.path().join("test_file.txt");
    fs::write(&file_path, "test content")?;

    let follow_id = get_file_id(&file_path)?;

    assert_eq!(follow_id, get_file_id_no_follow(&file_path)?);
    assert_eq!(
        follow_id,
        get_file_id_from_file(&fs::File::open(&file_path)?)?
    );

    Ok(())
}

// Following links has no effect on a regular directory's ID.
#[test]
fn test_directory_ids_match() -> io::Result<()> {
    let temp_dir = TempDir::new()?;
    let dir_path = temp_dir.path().join("test_dir");
    fs::create_dir(&dir_path)?;

    assert_eq!(get_file_id(&dir_path)?, get_file_id_no_follow(&dir_path)?);

    Ok(())
}

// Path and handle lookups distinguish two files in the same directory.
#[test]
fn test_different_files_have_different_ids() -> io::Result<()> {
    let temp_dir = TempDir::new()?;
    let a = temp_dir.path().join("a");
    let b = temp_dir.path().join("b");
    fs::write(&a, "a")?;
    fs::write(&b, "b")?;

    assert_ne!(get_file_id(&a)?, get_file_id(&b)?);
    assert_ne!(
        get_file_id_from_file(&fs::File::open(&a)?)?,
        get_file_id_from_file(&fs::File::open(&b)?)?
    );

    Ok(())
}

// An open handle returns the same ID as the file's new path after a rename.
#[test]
fn test_get_file_id_from_file_survives_rename() -> io::Result<()> {
    let temp_dir = TempDir::new()?;
    let old_path = temp_dir.path().join("old");
    let new_path = temp_dir.path().join("new");
    fs::write(&old_path, "test content")?;

    let file = fs::File::open(&old_path)?;
    fs::rename(&old_path, &new_path)?;

    assert_eq!(get_file_id_from_file(&file)?, get_file_id(&new_path)?);

    Ok(())
}

// Following a junction returns the target's ID; no-follow returns the junction's own ID.
#[cfg(target_family = "windows")]
#[test]
fn test_junction_id_differs_from_target() -> io::Result<()> {
    let temp_dir = TempDir::new()?;
    let target_path = temp_dir.path().join("target_dir");
    let junction_path = temp_dir.path().join("junction");
    fs::create_dir(&target_path)?;

    let status = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&junction_path)
        .arg(&target_path)
        .status()?;
    assert!(status.success(), "mklink /J failed: {status}");

    let target_id = get_file_id(&target_path)?;

    assert_eq!(target_id, get_file_id(&junction_path)?);
    assert_ne!(target_id, get_file_id_no_follow(&junction_path)?);

    Ok(())
}
