#![allow(unused)]
use std::io::Write;
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

    /// skip size check when children don't fit
    #[arg(short, long, default_value_t = false)]
    skip: bool,

    /// skip size check when children don't fit
    #[arg(short, long)]
    extract: Option<String>,
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
        skip: args.skip,
        extract: args.extract.clone(),
        root: args.directory.clone(),
    };
    proc.process_files_in_dir(&args.directory)
}

struct Processor {
    max: u32,
    skip: bool,
    extract: Option<String>,
    root: String,
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

                let relative_filename = if filename.starts_with(&self.root) {
                    let start_pos = self.root.len();
                    &filename[start_pos..]
                } else {
                    &filename
                };
                println!("======================= {relative_filename} =======================");

                self.analyse(&mut file, &format!("{relative_filename}_out"), 0, 0)?;
            }
        }

        Ok(())
    }

    fn analyse(&self, file: &mut File, output_file: &str, offset: u32, depth: u32) -> std::io::Result<()> {
        let header = self.read_header(file)?;
        let to_show = std::cmp::min(header.len(), self.max as usize);
        self.print(depth); println!("{:?}", &header[0..to_show]);
        let previous_header_size = header.len() * 4;
        let previous_header_size_and_offset = previous_header_size as u32 + offset;

        let mut header = self.read_header(file)?;
        let to_show = std::cmp::min(header.len(), self.max as usize);
        self.print(depth); println!("{:?}", &header[0..to_show]);

        let child_count = header[1];
        if child_count > 0 {
            let expected_size = (child_count * 2 + 5) as usize;

            if header.len() < expected_size {
                self.print(depth + 1); println!("{child_count} children but size is only {}.", header.len() * 4);

                let expected_size = (child_count * 2 + 5) as usize;
                while header.len() < expected_size {
                    header.push(file.read_u32::<LittleEndian>()?);
                }
            }

            for child in 0..child_count {
                let offset = header[(child + 5) as usize] + previous_header_size_and_offset;
                let size = header[(child + 5 + child_count) as usize];
                self.print(depth + 1); println!("{child}: from {:#X} to {:#X}", offset, offset + size);
                
                file.seek(SeekFrom::Start((offset) as u64))?;
                let mut magic = [0u8; 4];
                file.read(&mut magic)?;

                match magic {
                    [1, 0, 0, 0] => {
                        file.seek(SeekFrom::Current(-4))?;
                        let cur_pos = file.seek(SeekFrom::Current(0))? as u32;
                        self.analyse(file, output_file, cur_pos, depth + 1)?;
                    },
                    [0x74, 0x6D, 0x6F, 0x31] => {
                        self.print(depth + 2); println!("tmo1");
                        self.extract_file(file, output_file, offset, size);
                    },
                    [0x74, 0x6D, 0x64, 0x30] => {
                        self.print(depth + 2); println!("tmd0");
                        self.extract_file(file, output_file, offset, size);
                    },
                    [0x44, 0x44, 0x76, 0x20] => {
                        self.print(depth + 2); println!("dds1");
                        self.extract_file(file, output_file, offset, size);
                    },
                    _ => {
                        self.print(depth + 2);
                        let ascii = magic.into_iter().all(|x| x.is_ascii_graphic());
                        let maybe_str = String::from_utf8(magic.to_vec());
                        let magic = if ascii && maybe_str.is_ok() {
                            maybe_str.unwrap()
                        } else {
                            format!("{:?}", magic)
                        };
                        println!("unknown: {}", magic);
                        self.extract_file(file, output_file, offset, size);
                    },
                };
            }
        }

        Ok(())
    }

    fn extract_file(&self, file: &mut File, output_file: &str, start: u32, size: u32) -> std::io::Result<()> {
        if let Some(output_dir) = &self.extract {
            let save = file.seek(SeekFrom::Current(0))?;
            file.seek(SeekFrom::Start(start as u64))?;
            let mut buffer = vec![0u8; size as usize];
            file.read(&mut buffer)?;

            std::fs::create_dir_all(format!("{output_dir}/{output_file}"))?;

            let mut child_file = File::create(format!("{output_dir}/{output_file}/{:#X}.bin", start))?;
            child_file.write(&buffer);

            file.seek(SeekFrom::Start(save))?;
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

        for i in 4..(header[3] / 4) {
            header.push(file.read_u32::<LittleEndian>()?);
        }

        if self.skip {
            let child_count = header[1];
            let expected_size = (child_count * 2 + 5) as usize;
            while header.len() < expected_size {
                header.push(file.read_u32::<LittleEndian>()?);
            }
        }

        Ok(header)
    }
}
