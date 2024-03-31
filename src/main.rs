use image_dds::image::buffer::ConvertBuffer;
use byteorder::*;
use image_dds::image::io::Reader as ImageReader;
use crate::ddsfile::*;
use serde::Serialize;
use serde::Deserialize;
use image_dds::*;
use std::io::Write;
use std::io::SeekFrom;
use std::io::Seek;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::fs::File;
use clap::Parser;
use clap_derive::Parser;
use image_dds::image_from_dds;
use image_dds::image::*;
use image_dds::ddsfile::Dds;

#[derive(Debug)]
struct ChildData {
    offset: u64,
    size: u64,
}

#[derive(Parser, Debug)]
#[command(author = None, version = None, about = None, long_about = None)]
struct Args {
    /// Directory to scout for .cat files to analyse/extract or folder to pack into a .cat file (if --pack is set)
    input: String,

    /// The number of int to print for each header
    #[arg(short, long, default_value_t = 8)]
    max: u32,

    /// directory in which extract the assets
    #[arg(short, long)]
    extract: Option<String>,

    /// File into which the assets wille be packed
    #[arg(short, long)]
    pack: Option<String>,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let path = Path::new(&args.input);
    if !path.exists() {
        println!("'{}' does not exist.", args.input);
        return Ok(());
    }

    if !path.is_dir() {
        println!("'{}' is not a directory.", args.input);
        return Ok(());
    }

    let proc = Processor {
        extract: args.extract.clone(),
        root: if args.input.ends_with("/") {
            args.input.clone()
        } else {
            format!("{}/", args.input)
        },
    };
    if let Some(output) = args.pack {
        proc.pack_folder(&output)
    } else {
        proc.process_files_in_dir(&args.input)
    }
}

#[derive(Debug)]
struct ContainerHeader {
    format: u32,
    size: u32,
    alignment: u32,
    children: Vec<ChildData>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Container {
    format: u32,
    size: u32,
    alignment: u32,
    children: Vec<ArchiveEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Texture {
    name: String,
    format: DdsFormat,
    filename: String,
}

#[derive(Serialize, Deserialize, Debug)]
enum ArchiveEntry {
    Container(Container),
    ListOfTextures(Vec<Texture>),
    File(String),
    List { format: u32, files: Vec<String> },
    ListOfEntries(Vec<ArchiveEntry>),
}

struct Processor {
    extract: Option<String>,
    root: String,
}

use phf::{phf_map};

static FILE_FORMATS: phf::Map<u32, &'static str> = phf_map! {
    0x746D6430u32 => "tmd0",
    0x61303031u32 => "a001",
};

impl Processor {
    fn process_files_in_dir(&self, dir: &str) -> std::io::Result<()> {
        if !Path::new(dir).exists() {
            println!("'{dir}' does not exist");
            return Ok(());
        }

        for filepath in fs::read_dir(dir).unwrap() {
            let fpath = filepath?;
            if fpath.file_type()?.is_dir() {
                self.process_files_in_dir(fpath.path().to_str().unwrap())?;
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

                let container = match self.extract_container(&mut file, &format!("{relative_filename}_out"), 0) {
                    Ok(c) => c,
                    Err(e) => {
                        println!("{e}");
                        eprintln!("{relative_filename}: {e}");
                        if let Some(output_dir) = &self.extract {
                            fs::remove_dir_all(format!("{output_dir}/{relative_filename}_out"))?;
                        }
                        continue;
                    },
                };
                if let Some(output_dir) = &self.extract {
                    let ArchiveEntry::Container(container) = container else {
                        panic!("Root element isn't a container: {:?}", container);
                    };

                    std::fs::create_dir_all(format!("{output_dir}/{relative_filename}_out"))?;
                    let writer = File::create(format!("{output_dir}/{relative_filename}_out/metadata.json"))?;
                    serde_json::to_writer_pretty(writer, &container).unwrap();
                }
            }
        }

        Ok(())
    }

    fn display_header(&self, header: &ContainerHeader, depth: u32) {
        self.print(depth); println!("{:?}", header);
    }

    fn extract_container(&self, file: &mut File, output_file: &str, depth: u32) -> std::io::Result<ArchiveEntry> {
        let header = self.read_header(file)?;
        self.display_header(&header, depth);

        let mut children = vec![];
        if header.children.len() > 0 {
            let container_offset = self.offset(file);
            match header.format {
                0 => {
                    // the separating isn't strictly necessary, it's just to try to understand why different values
                    match (header.size, header.alignment) {
                        (256, 256) => {
                            for child in header.children.iter() {
                                let offset = child.offset + container_offset;
                                file.seek(SeekFrom::Start(offset))?;
                                children.push(self.extract_container(file, output_file, depth + 1)?);
                            }
                        },
                        (32, 16) => {
                            for (id, child) in header.children.iter().enumerate() {
                                let offset = child.offset + container_offset;
                                let size = child.size;

                                file.seek(SeekFrom::Start(offset))?;
                                // BigEndian so textual magic is left to right
                                let magic = file.read_u32::<BigEndian>()?;
                                file.seek(SeekFrom::Current(-4))?;
                                match magic {
                                    0x01000000 => {
                                        children.push(self.extract_container(file, output_file, depth + 1)?);
                                    },
                                    _ => {
                                        let ext = if FILE_FORMATS.contains_key(&magic) {
                                            FILE_FORMATS[&magic]
                                        } else {
                                            "bin"
                                        };
                                        let fileformat = format!("{:#X}.{ext}", offset);

                                        self.print(depth + 1); println!("{id}: from {:#X} to {:#X}", offset, offset + size);
                                        let magic_vec = self.extract_file(file, output_file, offset, size, Some(fileformat.clone()))?;
                                        if FILE_FORMATS.contains_key(&magic) {
                                            self.print(depth + 2); println!("{:?}", magic_vec);
                                        } else {
                                            self.print(depth + 2); println!("{}", fileformat);
                                        };
                                        children.push(ArchiveEntry::File(fileformat));
                                    },
                                }
                            }
                        },
                        _ => {
                            panic!("Unhandled header inside {output_file}: {}, {}", header.size, header.alignment);
                        },
                    };
                },
                2 => {
                    for child in header.children.iter() {
                        let offset = child.offset + container_offset;
                        file.seek(SeekFrom::Start(offset))?;
                        children.push(self.extract_format_2(file, output_file, depth + 1)?);
                    }
                },
                1 | 3 | 4 => {
                    children.push(self.extract_list(file, output_file, &header, depth + 1)?);
                },
                8 => {
                    assert_eq!(header.children.len(), 1);
                    children.push(self.extract_format_8(file, output_file, depth + 1)?);
                },
                _ => {
                    for (id, child) in header.children.iter().enumerate() {
                        let offset = child.offset + container_offset;
                        let size = child.size;
                        self.print(depth + 1); println!("{id}: from {:#X} to {:#X}", offset, offset + size);
                        let magic = self.extract_file(file, output_file, offset, size, None)?;
                        self.print(depth + 2); println!("{:?}", magic);
                        children.push(ArchiveEntry::File(format!("{:#X}.bin", offset)));
                    }
                },
            }
        }

        Ok(ArchiveEntry::Container(Container {
            format: header.format,
            size: header.size,
            alignment: header.alignment,
            children,
        }))
    }

    fn extract_format_2(&self, file: &mut File, output_file: &str, depth: u32) -> std::io::Result<ArchiveEntry> {
        let header = self.read_header(file)?;
        self.display_header(&header, depth);

        let filenames = if header.children[0].size > 0 {
            let fnames = self.read_filenames(file, &header.children[0])?;
            self.align(file, header.alignment);
            fnames
        } else {
            vec![]
        };

        let textures = if header.children[1].size > 0 {
            self.extract_block_of_files(file, output_file, depth, Some(filenames))?
        } else {
            vec![]
        };

        Ok(ArchiveEntry::ListOfTextures(textures))
    }

    fn extract_format_8(&self, file: &mut File, output_file: &str, depth: u32) -> std::io::Result<ArchiveEntry> {
        let header = self.read_header(file)?;
        assert_eq!(header.format, 0);
        self.display_header(&header, depth);

        let container_offset = self.offset(file);

        // let filenames = self.read_filenames(file, &header.children[0])?;

        let mut children = vec![];
        for child in header.children {
            let offset = child.offset + container_offset;
            let size = child.size;
            self.extract_file(file, output_file, offset, size, None)?;
            children.push(ArchiveEntry::File(format!("{:#X}.bin", offset)));
        }

        Ok(ArchiveEntry::Container(Container {
            format: header.format,
            size: header.size,
            alignment: header.alignment,
            children,
        }))
    }

    fn extract_list(&self, file: &mut File, output_file: &str, header: &ContainerHeader, depth: u32) -> std::io::Result<ArchiveEntry> {
        let file_offset = self.offset(file);

        let filenames = self.read_filenames(file, &header.children[0])?;
        self.align(file, header.alignment);

        let mut files = vec![];
        for (i, child) in header.children.iter().skip(1).enumerate() {
            let offset = child.offset + file_offset;
            let size = child.size;

            self.print(depth + 1); println!("{i}: from {:#X} to {:#X}", offset, offset + size);
            file.seek(SeekFrom::Start(offset))?;
            let magic = file.read_u32::<BigEndian>()?;
            file.seek(SeekFrom::Current(-4))?;
            let ext = if FILE_FORMATS.contains_key(&magic) {
                FILE_FORMATS[&magic]
            } else {
                "bin"
            };
            let filename = format!("{}.{ext}", filenames[i]);
            self.print(depth + 2); println!("{filename}");
            self.extract_file(file, output_file, offset, size, Some(filename.clone()))?;
            files.push(filename);
        }

        Ok(ArchiveEntry::List { format: header.format, files })
    }

    fn extract_block_of_files(&self, file: &mut File, output_file: &str, depth: u32, filenames: Option<Vec<String>>) -> std::io::Result<Vec<Texture>> {
        let file_pos = self.offset(file);
        let header_length = file.read_u32::<LittleEndian>()?;
        let child_count = file.read_u32::<LittleEndian>()?;

        if let Some(ref fnames) = filenames {
            if child_count != fnames.len() as u32 {
                println!("{child_count} // {:#X}: {:?}", file_pos, filenames);
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("123")));
            }
            //assert_eq!(child_count, fnames.len() as u32);
        }
        assert_eq!(header_length, (child_count + 3) * 4);

        let content_length = file.read_u32::<LittleEndian>()?;

        let first_pos = file_pos + (header_length + file.read_u32::<LittleEndian>()?) as u64;
        let mut children = vec![ChildData { offset: first_pos, size: 0 }];
        for _ in 1..child_count {
            let curr_file_offset = file_pos + (header_length + file.read_u32::<LittleEndian>()?) as u64;

            let last_offset = children.last().unwrap().offset;
            children.last_mut().unwrap().size = curr_file_offset - last_offset;
            children.push(ChildData { offset: curr_file_offset, size: 0 });
        }

        let content_end = file_pos + content_length as u64;
        let last_offset = children.last().unwrap().offset;
        children.last_mut().unwrap().size = content_end - last_offset;

        if let Some(ref fnames) = filenames {
            assert_eq!(fnames.len(), children.len());
        }

        let mut textures = vec![];
        for (i, child) in children.iter().enumerate() {
            file.seek(SeekFrom::Start(child.offset))?;
            let filename = if let Some(ref fnames) = filenames {
                fnames[i].to_string()
            } else {
                format!("{:#X}", child.offset)
            };
            self.print(depth + 1); println!("{i}: from {:#X} to {:#X}", child.offset, child.offset + child.size);
            self.print(depth + 2); println!("{filename}");

            let mut dds_buffer = vec![0u8; child.size as usize];
            file.read(&mut dds_buffer)?;
            let dds = Dds::read(&*dds_buffer).unwrap();
            if dds.get_d3d_format().is_none() {
                self.print(depth + 1); println!("{}: empty D3D, what is dxgi {:?}", filename, dds.get_dxgi_format());
            }
            textures.push(Texture {
                name: filename.clone(),
                format: d3d_to_dds(&dds.get_d3d_format().unwrap_or(D3DFormat::DXT1)),
                filename: filename.clone(),
            });
        }
            
        if let Some(output_dir) = &self.extract {
            std::fs::create_dir_all(format!("{output_dir}/{output_file}"))?;

            let save = file.seek(SeekFrom::Current(0))?;
            for (i, texture) in textures.iter_mut().enumerate() {
                file.seek(SeekFrom::Start(children[i].offset))?;
                let mut buffer = vec![0u8; children[i].size as usize];
                file.read(&mut buffer)?;
                let dds = Dds::read(&*buffer).unwrap();
                let image = image_from_dds(&dds, 0).unwrap();
                let new_filename = format!("{} ({}x{})", texture.filename, image.width(), image.height());
                image.save(&format!("{output_dir}/{output_file}/{}.png", new_filename)).unwrap();
                texture.filename = new_filename;
            }
            file.seek(SeekFrom::Start(save))?;
        }

        Ok(textures)
    }

    fn read_filenames(&self, file: &mut File, child_data: &ChildData) -> std::io::Result<Vec<String>> {
        let offset = self.offset(file);

        let strings_start = child_data.offset + offset;
        let strings_size = child_data.size;

        file.seek(SeekFrom::Start(strings_start as u64))?;
        let mut buffer = vec![0u8; strings_size as usize];
        file.read(&mut buffer)?;

        let Ok(strings) = String::from_utf8(buffer) else {
            panic!("offset: {:#X}", offset);
        };

        let filenames: Vec<_> = strings.split(",").map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();

        Ok(filenames)
    }

    fn extract_file(&self, file: &mut File, output_file: &str, start: u64, size: u64, name_override: Option<String>) -> std::io::Result<Vec<u8>> {
        let save = file.seek(SeekFrom::Current(0))?;
        file.seek(SeekFrom::Start(start as u64))?;
        let mut buffer = vec![0u8; size as usize];
        file.read(&mut buffer)?;

        if let Some(output_dir) = &self.extract {
            std::fs::create_dir_all(format!("{output_dir}/{output_file}"))?;

            let filename = if let Some(name_over) = name_override {
                name_over.to_string()
            } else {
                format!("{:#X}.bin", start)
            };
            let mut child_file = File::create(format!("{output_dir}/{output_file}/{filename}"))?;
            child_file.write(&buffer)?;
        }

        file.seek(SeekFrom::Start(save))?;
        Ok(if buffer.len() > 0 { buffer[0..4].to_vec() } else { vec![] })
    }

    fn print(&self, depth: u32) {
        print!("{}", " ".repeat(depth as usize * 2));
    }

    #[allow(unused)]
    fn pos(&self, file: &mut File) {
        println!("file position: {:#X}", self.offset(file));
    }

    fn offset(&self, file: &mut File) -> u64 {
        file.seek(SeekFrom::Current(0)).unwrap()
    }

    fn align(&self, file: &mut File, alignment: u32) {
        if alignment > 0 {
            let alignment = alignment as u64;
            let cur_pos = file.seek(SeekFrom::Current(0)).unwrap();
            if (cur_pos % alignment) != 0 {
                let cur_pos = (alignment - (cur_pos % alignment)) as i64;
                file.seek(SeekFrom::Current(cur_pos)).unwrap();
            }
        } else {
            println!("Alignement is NULL");
        }
    }

    fn read_header(&self, file: &mut File) -> std::io::Result<ContainerHeader> {
        let mut children = vec![];
        
        // part one
        let val = file.read_u32::<LittleEndian>()?; assert_eq!(val, 1);
        let val = file.read_u32::<LittleEndian>()?;
        if val != 1 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Can't handle '1, {val}, 0' containers ATM, only '1, 1, 0'. At pos {:#X}", self.offset(file) - 8)));
        }
        let val = file.read_u32::<LittleEndian>()?; assert_eq!(val, 0);
        let mut size = file.read_u32::<LittleEndian>()?;
        if size == 0 {
            size = 256;
            //return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Header size is NULL: {:#X}", self.offset(file) - 4)));
        }
        
        assert!(size >= 32);
        // maybe check the duplicate values?
        let byte_zero = file.seek(SeekFrom::Current(size as i64 - 16))?;

        // part two
        let val = file.read_u32::<LittleEndian>()?; assert_eq!(val, 0);
        let child_count = file.read_u32::<LittleEndian>()? as usize;
        let format = file.read_u32::<LittleEndian>()?;
        let alignment = file.read_u32::<LittleEndian>()?;
        let val = file.read_u32::<LittleEndian>()?; assert_eq!(val, 0);

        let mut data = vec![];
        for _ in 0..child_count {
            data.push(file.read_u32::<LittleEndian>()?);
            data.push(file.read_u32::<LittleEndian>()?);
        }

        self.align(file, alignment);

        let header_offset = self.offset(file) - byte_zero;
        for i in 0..(child_count) {
            let offset = data[i] as u64 - header_offset;
            let size = data[i + child_count] as u64;
            children.push(ChildData {
                offset, size,
            });
        }

        Ok(ContainerHeader {
            size,
            format,
            alignment,
            children,
        })
    }

    fn pack_folder(&self, output_file: &str) -> std::io::Result<()> {
        if !Path::new(&format!("{}metadata.json", self.root)).exists() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{}metadata.json", self.root)));
        }

        let strbuf = fs::read_to_string(format!("{}metadata.json", self.root)).unwrap();
        let archive: Container = serde_json::from_str(&strbuf).unwrap();

        let mut file = File::create(output_file)?;
        self.pack_container(&mut file, &archive)?;

        Ok(())
    }

    fn pack_container(&self, file: &mut File, container: &Container) -> std::io::Result<Vec<ChildData>> {
        let addr = file.seek(SeekFrom::Current(0))?;

        file.write_u32::<LittleEndian>(1)?;
        file.write_u32::<LittleEndian>(1)?;
        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(container.size)?;
        let container_size_addr = file.seek(SeekFrom::Current(0))?;
        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(container.format)?;
        file.write_u32::<LittleEndian>(0)?;
        for _ in 0..(container.size / 4 - 7) {
            file.write_u32::<LittleEndian>(0)?;
        }

        file.write_u32::<LittleEndian>(0)?;
        let child_count_addr = file.seek(SeekFrom::Current(0))?;
        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(container.format)?;
        file.write_u32::<LittleEndian>(container.alignment)?;
        file.write_u32::<LittleEndian>(0)?;
        let children_offsets_addr = file.seek(SeekFrom::Current(0))?;

        let mut real_children_count = 0;
        for child in container.children.iter() {
            real_children_count += match child {
                ArchiveEntry::List { .. } => 2,
                ArchiveEntry::ListOfTextures(_) => 1,
                ArchiveEntry::ListOfEntries(entries) => entries.len(),
                ArchiveEntry::Container(_) => 1,
                ArchiveEntry::File(_) => 1,
            };
        }

        for _ in 0..(real_children_count * 2) {
            file.write_u32::<LittleEndian>(0)?;
        }

        let pos_after_padding = file.seek(SeekFrom::Current(0))?;
        let mut padding = pos_after_padding as u32;
        if padding % container.alignment > 0 {
            padding = container.alignment - (padding % container.alignment);
            for _ in 0..padding {
                file.write_u8(0)?;
            }
        }

        let mut all_children = vec![];
        for child in container.children.iter() {
            all_children.extend(match child {
                ArchiveEntry::List { format: _, files } => self.pack_list(file, files)?,
                ArchiveEntry::ListOfTextures(textures) => self.pack_block(file, textures)?,
                ArchiveEntry::ListOfEntries(entries) => self.pack_list_of_blocks(file, entries)?,
                ArchiveEntry::Container(container) => self.pack_container(file, container)?,
                ArchiveEntry::File(filename) => self.pack_file(file, filename)?,
            });

            let alignment = container.alignment as u64;
            let curr_addr = file.seek(SeekFrom::Current(0))?;
            if curr_addr % alignment > 0 {
                let padding = alignment - (curr_addr % alignment);
                for _ in 0..padding {
                    file.write_u8(0)?;
                }
            }
        }

        let cur_pos = file.seek(SeekFrom::Current(0))?;

        file.seek(SeekFrom::Start(container_size_addr))?;
        file.write_u32::<LittleEndian>((cur_pos - addr - 256) as u32)?;

        file.seek(SeekFrom::Current(4))?;
        file.write_u32::<LittleEndian>(all_children.len() as u32)?;
        file.seek(SeekFrom::Start(child_count_addr))?;
        file.write_u32::<LittleEndian>(all_children.len() as u32)?;

        file.seek(SeekFrom::Start(children_offsets_addr))?;
        for c in &all_children {
            file.write_u32::<LittleEndian>((c.offset - child_count_addr + 4) as u32)?;
        }
        for c in &all_children {
            file.write_u32::<LittleEndian>(c.size as u32)?;
        }

        file.seek(SeekFrom::Start(cur_pos))?;

        let size_of_block = file.seek(SeekFrom::Current(0))? - addr;

        Ok(vec![ChildData {
            offset: addr,
            size: size_of_block,
        }])
    }

    fn pack_file(&self, file: &mut File, filename: &str) -> std::io::Result<Vec<ChildData>> {
        let start = file.seek(SeekFrom::Current(0))?;
        let filename = format!("{}{}", self.root, filename);
        let data = fs::read(filename)?;
        file.write(&data)?;

        Ok(vec![ChildData {
            offset: start,
            size: data.len() as u64,
        }])
    }

    fn pack_list(&self, file: &mut File, files: &Vec<String>) -> std::io::Result<Vec<ChildData>> {
        let addr = file.seek(SeekFrom::Current(0))?;
        let mut children_data = vec![ChildData { offset: addr, size: 0 }];

        let strings = files.iter().map(|name| {
            if let Some(pos) = name.rfind(".") {
                format!("{}", &name[0..pos])
            } else {
                name.clone()
            }
        }).collect::<Vec<_>>().join(",\r\n");
        write!(file, "{},\r\n", strings)?;
        let padding = file.seek(SeekFrom::Current(0))?;
        children_data.last_mut().unwrap().size = padding - addr;

        if padding % 256 > 0 {
            let padding = 256 - (padding % 256);
            for _ in 0..padding {
                file.write_u8(0)?;
            }
        }

        let files_start = file.seek(SeekFrom::Current(0))?;
        children_data.push(ChildData {
            offset: files_start, size: 0
        });
        for f in files.iter() {
            let filename = format!("{}{}", self.root, f);
            let data = fs::read(filename)?;
            file.write(&data)?;
        }
        let files_end = file.seek(SeekFrom::Current(0))?;
        children_data.last_mut().unwrap().size = files_end - files_start;

        Ok(children_data)
    }

    fn pack_list_of_blocks(&self, file: &mut File, entries: &Vec<ArchiveEntry>) -> std::io::Result<Vec<ChildData>> {

        let mut all_children = vec![];

        for entry in entries.iter() {
            match entry {
                ArchiveEntry::ListOfTextures(textures) => {
                    all_children.extend(self.pack_block(file, textures)?);
                },
                _ => panic!(""),
            };
        }

        Ok(all_children)
    }

    fn pack_block(&self, file: &mut File, textures: &Vec<Texture>) -> std::io::Result<Vec<ChildData>> {
        let start_of_block = file.seek(SeekFrom::Current(0))?;

        file.write_u32::<LittleEndian>(1)?;
        file.write_u32::<LittleEndian>(1)?;
        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(256)?;
        file.write_u32::<LittleEndian>(0x42424242)?;
        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(2)?;
        for _ in 0..(64 - 7) {
            file.write_u32::<LittleEndian>(0)?;
        }

        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(2)?;
        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(256)?;
        file.write_u32::<LittleEndian>(0)?;
        // offsets
        file.write_u32::<LittleEndian>(0x69696969)?;
        file.write_u32::<LittleEndian>(0x69696969)?;
        // sizes
        file.write_u32::<LittleEndian>(0x69696969)?;
        file.write_u32::<LittleEndian>(0x69696969)?;
        for _ in 0..(64 - 9) {
            file.write_u32::<LittleEndian>(0)?;
        }

        let data_begin_addr = file.seek(SeekFrom::Current(0))?;
        //let mut files_offset = vec![data_begin_addr];
        let mut files_size = vec![];

        let strings = textures.iter().map(|tex| tex.name.clone()).collect::<Vec<_>>().join(",\r\n");
        let strings = format!("{strings},\r\n");
        write!(file, "{}", strings)?;
        for _ in 0..(256 - strings.len()) {
            file.write_u8(0)?;
        }

        files_size.push(strings.len() as u64);
        let textures_begin_addr = file.seek(SeekFrom::Current(0))?;

        file.seek(SeekFrom::Start(data_begin_addr - (256 - 20)))?;
        file.write_u32::<LittleEndian>(256)?;
        file.write_u32::<LittleEndian>(256 + (textures_begin_addr - data_begin_addr) as u32)?;
        file.write_u32::<LittleEndian>(strings.len() as u32)?;

        file.seek(SeekFrom::Start(textures_begin_addr))?;
        let child_count = textures.len() as u32;
        let header_length = (child_count + 3) * 4;
        file.write_u32::<LittleEndian>(header_length)?;
        file.write_u32::<LittleEndian>(child_count)?;

        let block_size_addr = file.seek(SeekFrom::Current(0))?;
        file.write_u32::<LittleEndian>(0x55555555)?;
        // offsets
        for _ in 0..textures.len() {
            file.write_u32::<LittleEndian>(0x77777777)?;
        }

        let mut textures_offset = vec![];
        let mut textures_size = vec![];
        for tex in textures {
            let texture_begin_addr = file.seek(SeekFrom::Current(0))?;

            let filename = format!("{}{}.png", self.root, tex.filename);
            let img = ImageReader::open(&filename).unwrap().decode().unwrap();
            let img = match img {
                DynamicImage::ImageRgba8(image) => image,
                DynamicImage::ImageRgb8(image) => {
                    let rgba_image: RgbaImage = image.convert();
                    rgba_image
                },
                _ => panic!("{} is not a RGB(A) image: {:?}", filename, img),
            };

            let d3dformat = dds_to_d3d(&tex.format);
            let dds = internal::dds_from_image(&img, d3dformat).unwrap();
            dds.write(file).unwrap();
            let cur_pos = file.seek(SeekFrom::Current(0))?;

            textures_offset.push(texture_begin_addr);
            textures_size.push(cur_pos - texture_begin_addr);
        }

        let saved_pos = file.seek(SeekFrom::Current(0))?;

        let last_addr = textures_offset.last().unwrap() + textures_size.last().unwrap() - textures_offset[0];
        file.seek(SeekFrom::Start(block_size_addr))?;
        let full_size = last_addr as u32 + header_length as u32;
        file.write_u32::<LittleEndian>(full_size)?;

        for off in &textures_offset {
            file.write_u32::<LittleEndian>((off - textures_offset[0]) as u32)?;
        }

        file.seek(SeekFrom::Start(data_begin_addr - (256 - 32)))?;
        file.write_u32::<LittleEndian>(full_size)?;

        file.seek(SeekFrom::Start(data_begin_addr - (512 - 16)))?;
        let saved_pos_mod = 0x100 - (saved_pos % 0x100);
        let cont_size = saved_pos - (data_begin_addr - saved_pos_mod) + 256;
        file.write_u32::<LittleEndian>(cont_size as u32)?;

        file.seek(SeekFrom::Start(saved_pos))?;

        if saved_pos % 256 > 0 {
            let padding = 256 - (saved_pos % 256);
            for _ in 0..padding {
                file.write_u8(0)?;
            }
        }

        let size_of_block = file.seek(SeekFrom::Current(0))? - start_of_block;

        Ok(vec![ChildData {
            offset: start_of_block,
            size: size_of_block,
        }])
    }
}

#[derive(Serialize, Deserialize, Debug)]
enum DdsFormat {
    Dxt1,
    Dxt3,
    Dxt5,
    A8R8G8B8,
}

fn d3d_to_dds(format: &D3DFormat) -> DdsFormat {
    match format {
        D3DFormat::DXT1 => DdsFormat::Dxt1,
        D3DFormat::DXT3 => DdsFormat::Dxt3,
        D3DFormat::DXT5 => DdsFormat::Dxt5,
        D3DFormat::A8R8G8B8 => DdsFormat::A8R8G8B8,
        _ => panic!("Unhandled format: {:?}", format),
    }
}

fn dds_to_d3d(format: &DdsFormat) -> D3DFormat {
    match format {
        DdsFormat::Dxt1 => D3DFormat::DXT1,
        DdsFormat::Dxt3 => D3DFormat::DXT1, // image_dds doesn't support DXT3
        DdsFormat::Dxt5 => D3DFormat::DXT5,
        DdsFormat::A8R8G8B8 => D3DFormat::A8R8G8B8,
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
        D3DFormat::A8R8G8B8 => ImageFormat::Rgba8UnormSrgb,
        _ => {
            println!("{:?}", format);
            unimplemented!();
        },
    };
    SurfaceRgba8::from_image(image)
        .encode(other_format, Quality::Normal, Mipmaps::Disabled)?
        .to_d3d_dds(format)
}

}
