#![allow(unused)]
use std::io::SeekFrom;
use std::io::Seek;
use std::fs;
use std::io::Read;
use std::path::Path;
use byteorder::ReadBytesExt;
use byteorder::LittleEndian;
use std::fs::File;

fn main() -> std::io::Result<()> {
    let directory = match std::env::args().skip(1).next() {
        Some(f) => f,
        None => {
            println!("Missing file argument.");
            return Ok(());
        }
    };

    let path = Path::new(&directory);
    if !path.exists() {
        println!("'{directory}' does not exist.");
        return Ok(());
    }

    if !path.is_dir() {
        println!("'{directory}' is not a directory.");
        return Ok(());
    }

    process_files_in_dir(&directory)
}

fn process_files_in_dir(dir: &str) -> std::io::Result<()> {
    if !Path::new(&dir).exists() {
        println!("'{dir}' does not exist");
        return Ok(());
    }

    for filepath in fs::read_dir(dir).unwrap() {
        let fpath = filepath?;
        if fpath.file_type()?.is_dir() {
            process_files_in_dir(fpath.path().to_str().unwrap());
        } else if fpath.path().to_str().unwrap().ends_with(".cat") {
            let filename = fpath.path().to_str().unwrap().to_string();
            let mut file = File::open(&filename)?;
            let mut header_starting = 0;
            print!("{filename}: ", );
            for h in 0..4 {
                let mut header = [0u32; 4];
                for i in 0..4 {
                    header[i] = file.read_u32::<LittleEndian>()?;
                }

                print!("h{h}: {:?} ", header);

                if header[3] != 0 {
                    header_starting += header[3];
                    file.seek(SeekFrom::Start(header_starting as u64))?;
                }
            }
            println!("");
        }
    }

    Ok(())
}
