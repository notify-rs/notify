use std::io;

use file_id::FileId;

fn main() {
    let path = std::env::args().nth(1).expect("no path given");

    print_file_id(&path);
}

#[cfg(target_family = "unix")]
fn print_file_id(path: &str) {
    print_result(file_id::get_file_id(path));
}

#[cfg(target_family = "windows")]
fn print_file_id(path: &str) {
    print_result(file_id::get_low_res_file_id(path));
    print_result(file_id::get_high_res_file_id(path));
}

fn print_result(result: io::Result<FileId>) {
    match result {
        Ok(file_id) => println!("{file_id:?}"),
        Err(error) => println!("Error: {error}"),
    }
}
