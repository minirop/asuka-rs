#![allow(unused)]
use byteorder::BigEndian;
use clap_derive::ValueEnum;
use std::io::SeekFrom;
use std::io::Seek;
use std::fs;
use std::io::Read;
use std::path::Path;
use byteorder::ReadBytesExt;
use byteorder::LittleEndian;
use std::fs::File;
use clap::Parser;
use clap_derive::Parser;
use clap_derive::Args;

#[derive(Parser, Debug)]
#[command(author = None, version = None, about = None, long_about = None)]
struct Args {
    directory: String,

    /// The number of int to print for each header
    #[arg(short, long, default_value_t = 8)]
    max: u32,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let path = Path::new(&args.directory);
    if !path.exists() {
        println!("'{}' does not exist.", args.directory);
        return Ok(());
    }

    if !path.is_dir() {
        println!("'{}' is not a directory.", args.directory);
        return Ok(());
    }

    let mut proc = Processor {
        max: args.max,
    };
    proc.process_files_in_dir(&args.directory)
}

struct Processor {
    max: u32,
}

impl Processor {
    fn process_files_in_dir(&self, dir: &str) -> std::io::Result<()> {
        if !Path::new(dir).exists() {
            println!("'{dir}' does not exist");
            return Ok(());
        }

        for filepath in fs::read_dir(dir).unwrap() {
            let fpath = filepath?;
            if fpath.file_type()?.is_dir() {
                self.process_files_in_dir(fpath.path().to_str().unwrap());
            } else if fpath.path().to_str().unwrap().ends_with(".cat") {
                let filename = fpath.path().to_str().unwrap().to_string();
                let mut file = File::open(&filename)?;
                println!("======================= {filename} =======================");

                self.analyse(&mut file, 0, 0)?;
            }
        }

        Ok(())
    }

    fn analyse(&self, file: &mut File, offset: u32, depth: u32) -> std::io::Result<()> {
        let header = self.read_header(file)?;
        let to_show = std::cmp::min(header[3] / 4, self.max) as usize;
        self.print(depth); println!("{:?}", &header[0..to_show]);
        let previous_header_size = header[3] + offset;
        let header = self.read_header(file)?;
        let to_show = std::cmp::min(header[3] / 4, self.max) as usize;
        self.print(depth); println!("{:?}", &header[0..to_show]);

        let child_count = header[1];
        if child_count * 8 + 5 <= header.len() as u32 {
            for child in 0..child_count {
                let offset = header[(child + 5) as usize];
                let size = header[(child + 5 + child_count) as usize];
                self.print(depth); println!("{child}: from {:#X} to {:#X}", offset + previous_header_size, offset + size + previous_header_size);

                file.seek(SeekFrom::Start((offset + previous_header_size) as u64))?;
                let magic = file.read_u32::<BigEndian>()?;
                match magic {
                    0x01000000 => {
                        file.seek(SeekFrom::Current(-4))?;
                        let cur_pos = file.seek(SeekFrom::Current(0))? as u32;
                        self.analyse(file, cur_pos, depth + 1)?;
                    },
                    0x746D6F31 => { self.print(depth + 1); println!("tmo1"); },
                    0x44447620 => { self.print(depth + 1); println!("dds1"); },
                    _ => { self.print(depth + 1); println!("unknown: {:#X}", magic); },
                };
            }
        } else {
            self.print(depth + 1); println!("{child_count} children but size is only {}.", header.len());
            let _ = file.read_u32::<LittleEndian>()?;
            let mut childrens = vec![];
            for i in 0..child_count {
                childrens.push(file.read_u32::<LittleEndian>()?);
            }
            for i in 0..child_count {
                childrens.push(file.read_u32::<LittleEndian>()?);
            }

            for i in 0..child_count {
                let start = childrens[i as usize] + previous_header_size;
                let size = childrens[(i + child_count) as usize];
                self.print(depth + 2); println!("child {i}: from {:#X} to {:#X}", start, start + size);
            }
        }

        Ok(())
    }

    fn print(&self, depth: u32) {
        print!("{}", " ".repeat(depth as usize * 2));
    }

    fn read_header(&self, file: &mut File) -> std::io::Result<Vec<u32>> {
        let mut header = vec![];
        
        for _ in 0..4 {
            header.push(file.read_u32::<LittleEndian>()?);
        }

        let to_read = std::cmp::min(header[3] / 4, self.max);
        for i in 4..(header[3] / 4) {
            let int = file.read_u32::<LittleEndian>()?;
            header.push(int);
        }

        Ok(header)
    }
}
