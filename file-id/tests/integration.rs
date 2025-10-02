use file_id::{get_file_id, get_file_id_no_follow};
use std::{fs, io};
use tempfile::TempDir;

#[test]
fn test_get_file_id_vs_no_follow() -> io::Result<()> {
    let temp_dir = TempDir::new()?;
    let file_path = temp_dir.path().join("test_file.txt");
    let symlink_path = temp_dir.path().join("test_symlink");

    // Create a test file
    fs::write(&file_path, "test content")?;

    // Create a symlink to the file
    #[cfg(target_family = "unix")]
    std::os::unix::fs::symlink(&file_path, &symlink_path)?;

    #[cfg(target_family = "windows")]
    std::os::windows::fs::symlink_file(&file_path, &symlink_path)?;

    // Get file IDs
    let original_file_id = get_file_id(&file_path)?;
    let symlink_follow_id = get_file_id(&symlink_path)?;
    let symlink_no_follow_id = get_file_id_no_follow(&symlink_path)?;

    // Following the symlink should give us the same ID as the original file
    assert_eq!(original_file_id, symlink_follow_id);

    // Not following the symlink should give us a different ID (the symlink's own ID)
    assert_ne!(original_file_id, symlink_no_follow_id);

    Ok(())
}
