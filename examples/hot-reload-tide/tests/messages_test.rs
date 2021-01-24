use announcer::messages::*;

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

#[test]
fn save_config_to_file() {
    let config = Config {
        audio_folder_path: "sounds/".to_string(),
        messages: [(
            "sound2.mp3".to_string(),
            Message {
                display_name: "Sound 2".to_string(),
                volume: 60_f32,
            },
        )]
        .iter()
        .cloned()
        .collect(),
    };

    let path = "tests/messages_test_config_saved.json";

    save_config(config, path);

    let Config {
        audio_folder_path,
        messages,
    } = load_config(path).unwrap();

    assert_eq!(audio_folder_path, "sounds/");

    let message = messages.get("sound2.mp3").unwrap();

    assert_eq!(message.display_name, "Sound 2");
}
