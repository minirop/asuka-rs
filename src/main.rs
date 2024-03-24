#![allow(unused)]
use crate::ddsfile::*;
use serde::Serialize;
use serde::Deserialize;
use image_dds::*;
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
use image_dds::image_from_dds;
use image_dds::ddsfile::Dds;

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
        root: if args.directory.ends_with("/") {
            args.directory.clone()
        } else {
            format!("{}/", args.directory)
        },
    };
    proc.process_files_in_dir(&args.directory)
}

#[derive(Serialize, Deserialize, Debug)]
struct Container {
    format: u32,
    header_size: u32,
    subheader_size: u32,
    children: Vec<ArchiveEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Texture {
    name: String,
    format: DdsFormat,
}

#[derive(Serialize, Deserialize, Debug)]
enum ArchiveEntry {
    Container(Container),
    ListOfTextures(Vec<Texture>),
    File(String),
    List { id: u32, files: Vec<String> },
}

#[derive(Serialize, Deserialize, Debug)]
struct CatArchive {
    root: Container,
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

                let container = self.analyse(&mut file, &format!("{relative_filename}_out"), 0, 0)?;
                if let Some(output_dir) = &self.extract {
                    let writer = File::create(format!("{output_dir}/{relative_filename}_out/metadata.json"))?;
                    serde_json::to_writer_pretty(writer, &container).unwrap();
                }
            }
        }

        Ok(())
    }

    fn display_header(&self, header: &Vec<u32>, depth: u32) {
        let to_show = std::cmp::min(header.len(), self.max as usize);
        self.print(depth); println!("{:?}", &header[0..to_show]);
    }

    fn analyse(&self, file: &mut File, output_file: &str, offset: u32, depth: u32) -> std::io::Result<Container> {
        let header = self.read_header(file)?;
        self.display_header(&header, depth);
        let previous_header_size = header.len() * 4;
        let previous_header_size_and_offset = previous_header_size as u32 + offset;
        let header_size = header[3];

        let mut header = self.read_header(file)?;
        self.display_header(&header, depth);
        let subheader_size = header[3];
        let format = header[2];
        let mut children = vec![];

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

            match header[2] {
                1 | 3 | 4 | 6 | 7 => {
                    let child = self.extract_list(file, output_file, previous_header_size_and_offset, depth + 1, &header, header[2])?;
                    children.push(child);
                },
                2 => {
                    let child = self.extract_type_2(file, output_file, previous_header_size_and_offset, depth + 1)?;
                    children.push(child);
                },
                _ => {
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

                                let child = self.analyse(file, output_file, cur_pos, depth + 1)?;
                                children.push(ArchiveEntry::Container(child));
                            },
                            [0x74, 0x6D, 0x6F, 0x31] => {
                                self.print(depth + 2); println!("tmo1");
                                let name_override = &format!("{:#X}.tmo", offset);
                                self.extract_file(file, output_file, offset, size, Some(name_override))?;
                                children.push(ArchiveEntry::File(name_override.to_string()));
                            },
                            [0x74, 0x6D, 0x64, 0x30] => {
                                self.print(depth + 2); println!("tmd0");
                                let name_override = &format!("{:#X}.tmd", offset);
                                self.extract_file(file, output_file, offset, size, Some(name_override))?;
                                children.push(ArchiveEntry::File(name_override.to_string()));
                            },
                            [0x44, 0x44, 0x76, 0x20] => {
                                self.print(depth + 2); println!("dds1");
                                let name_override = &format!("{:#X}.dds", offset);
                                self.extract_file(file, output_file, offset, size, Some(name_override))?;
                                children.push(ArchiveEntry::File(name_override.to_string()));
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
                                self.extract_file(file, output_file, offset, size, None)?;
                                children.push(ArchiveEntry::File(format!("{:#X}.bin", offset)));
                            },
                        };
                    }
                },
            };
        }

        Ok(Container {
            format,
            header_size,
            subheader_size,
            children,
        })
    }

    fn extract_list(&self, file: &mut File, output_file: &str, offset: u32, depth: u32, header: &Vec<u32>, id: u32) -> std::io::Result<ArchiveEntry> {
        let strings_start = header[5] + offset;
        let strings_size = header[7];

        file.seek(SeekFrom::Start(strings_start as u64))?;
        let mut buffer = vec![0u8; strings_size as usize];
        file.read(&mut buffer)?;
        let strings = String::from_utf8(buffer).unwrap();
        let filenames: Vec<_> = strings.split(",\r\n").filter(|e| !e.is_empty()).collect();

        let mut files = vec![];

        for i in 0..filenames.len() {
            let offset = header[6 + i] + offset;
            let size = header[6 + i + 1 + filenames.len()];
            self.print(depth); println!("{}: from {:#X} to {:#X}", filenames[i], offset, offset + size);

            file.seek(SeekFrom::Start((offset) as u64))?;
            let mut magic = [0u8; 4];
            file.read(&mut magic)?;

            let extension = match magic {
                [0x74, 0x6D, 0x6F, 0x31] => "tmo1",
                [0x74, 0x6D, 0x64, 0x30] => "tmd0",
                _ => panic!("Unknown file magic: {:?}", magic),
            };
            let name_override = &format!("{:#X}.{}", offset, extension);
            files.push(name_override.to_string());

            if let Some(output_dir) = &self.extract {
                self.extract_file(file, output_file, offset, size, Some(name_override))?;
            }
        }

        Ok(ArchiveEntry::List { id, files })
    }

    fn extract_type_2(&self, file: &mut File, output_file: &str, offset: u32, depth: u32) -> std::io::Result<ArchiveEntry> {
        let header = self.read_header(file)?;
        self.display_header(&header, depth);
        let header = self.read_header(file)?;
        self.display_header(&header, depth);
        let offset = offset + 0x200;

        let strings_start = header[5] + offset;
        let strings_size = header[7];
        let files_start = header[6] + offset;
        let files_size = header[8];

        file.seek(SeekFrom::Start(strings_start as u64))?;
        let mut buffer = vec![0u8; strings_size as usize];
        file.read(&mut buffer)?;
        let strings = String::from_utf8(buffer).unwrap();
        let filenames: Vec<_> = strings.split(",\r\n").filter(|e| !e.is_empty()).collect();

        let file_pos = file.seek(SeekFrom::Start(files_start as u64))?;
        let header_length = file.read_u32::<LittleEndian>()?;
        let child_count = file.read_u32::<LittleEndian>()?;
        assert_eq!(child_count, filenames.len() as u32);
        assert_eq!(header_length, (child_count + 3) * 4);
        let content_length = file.read_u32::<LittleEndian>()?;

        let first_pos = file_pos + (header_length + file.read_u32::<LittleEndian>()?) as u64;
        let mut files_offset = vec![first_pos];
        let mut files_size = vec![];
        for _ in 1..filenames.len() {
            let curr_file_offset = file_pos + (header_length + file.read_u32::<LittleEndian>()?) as u64;
            files_size.push(curr_file_offset - files_offset.last().unwrap());
            files_offset.push(curr_file_offset);
        }
        let content_end = file_pos + content_length as u64;
        files_size.push(content_end - files_offset.last().unwrap());
        assert_eq!(filenames.len(), files_offset.len());
        assert_eq!(filenames.len(), files_size.len());

        let mut textures = vec![];
        for i in 0..filenames.len() {
            self.print(depth + 1); println!("{}: from {:#X} to {:#X}", filenames[i], files_offset[i], files_offset[i] + files_size[i]);
            file.seek(SeekFrom::Start(files_offset[i] as u64))?;
            let dds = Dds::read(&*file).unwrap();
            textures.push(Texture {
                name: filenames[i].to_string(),
                format: d3d_to_dds(&dds.get_d3d_format().unwrap()),
            });
        }

        if let Some(output_dir) = &self.extract {
            std::fs::create_dir_all(format!("{output_dir}/{output_file}"))?;

            for i in 0..filenames.len() {
                file.seek(SeekFrom::Start(files_offset[i] as u64))?;
                let dds = Dds::read(&*file).unwrap();
                let image = image_from_dds(&dds, 0).unwrap();
                image.save(&format!("{output_dir}/{output_file}/{}.png", filenames[i])).unwrap();
            }
        }

        Ok(ArchiveEntry::ListOfTextures(textures))
    }

    fn extract_file(&self, file: &mut File, output_file: &str, start: u32, size: u32, name_override: Option<&str>) -> std::io::Result<()> {
        if let Some(output_dir) = &self.extract {
            let save = file.seek(SeekFrom::Current(0))?;
            file.seek(SeekFrom::Start(start as u64))?;
            let mut buffer = vec![0u8; size as usize];
            file.read(&mut buffer)?;

            std::fs::create_dir_all(format!("{output_dir}/{output_file}"))?;

            let filename = if let Some(name_over) = name_override {
                name_over.to_string()
            } else {
                format!("{:#X}.bin", start)
            };
            let mut child_file = File::create(format!("{output_dir}/{output_file}/{filename}"))?;
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

#[derive(Serialize, Deserialize, Debug)]
enum DdsFormat {
    Dxt1,
    Dxt3,
    Dxt5,
}

fn d3d_to_dds(format: &D3DFormat) -> DdsFormat {
    match format {
        D3DFormat::DXT1 => DdsFormat::Dxt1,
        D3DFormat::DXT3 => DdsFormat::Dxt3,
        D3DFormat::DXT5 => DdsFormat::Dxt5,
        _ => panic!("Unhandled format: {:?}", format),
    }
}

fn dds_to_d3d(format: &DdsFormat) -> D3DFormat {
    match format {
        DdsFormat::Dxt1 => D3DFormat::DXT1,
        DdsFormat::Dxt3 => D3DFormat::DXT3,
        DdsFormat::Dxt5 => D3DFormat::DXT5,
    }
}

trait ToD3dDss {
    fn to_d3d_dds(&self, format: D3DFormat) -> Result<image_dds::ddsfile::Dds, CreateDdsError>;
}

impl<T: AsRef<[u8]>> ToD3dDss for Surface<T> {
    fn to_d3d_dds(&self, format: D3DFormat) -> Result<crate::ddsfile::Dds, CreateDdsError> {
        let mut dds = Dds::new_d3d(ddsfile::NewD3dParams {
            height: self.height,
            width: self.width,
            depth: None,
            format,
            mipmap_levels: None,
            caps2: None,
        })?;

        dds.data = self.data.as_ref().to_vec();

        Ok(dds)
    }
}

mod internal {

use crate::ToD3dDss;
use image_dds::SurfaceRgba8;
use image_dds::*;
use ddsfile::Dds;
use ddsfile::D3DFormat;

pub fn dds_from_image(
    image: &image::RgbaImage,
    format: D3DFormat,
) -> Result<Dds, CreateDdsError> {
    let other_format = match format {
        D3DFormat::DXT1 => ImageFormat::BC1RgbaUnormSrgb,
        D3DFormat::DXT3 => ImageFormat::BC2RgbaUnormSrgb,
        D3DFormat::DXT5 => ImageFormat::BC3RgbaUnormSrgb,
        _ => unimplemented!(),
    };
    SurfaceRgba8::from_image(image)
        .encode(other_format, Quality::Normal, Mipmaps::Disabled)?
        .to_d3d_dds(format)
}

}
