use hot_reload_tide::messages::{load_config, Config};

#[test]
fn load_config_from_file() {
    let Config {
        audio_folder_path,
        messages,
    } = load_config("tests/messages_test_config.json").unwrap();

    assert_eq!(audio_folder_path, "sounds/");

    let message = messages.get("sound.mp3").unwrap();

    assert_eq!(message.display_name, "Sound 1");
}
