#![allow(unused)]
use image_dds::image::buffer::ConvertBuffer;
use std::env::args;
use byteorder::*;
use image_dds::image::io::Reader as ImageReader;
use crate::ddsfile::*;
use serde::Serialize;
use serde::Deserialize;
use image_dds::*;
use std::io::Write;
use clap_derive::ValueEnum;
use std::io::SeekFrom;
use std::io::Seek;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::fs::File;
use clap::Parser;
use clap_derive::Parser;
use clap_derive::Args;
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

    let mut proc = Processor {
        max: args.max,
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

#[derive(Serialize, Deserialize, Debug)]
struct Container {
    format: u32,
    header_size: u32,
    children_alignment: u32,
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
    List { id: u32, files: Vec<String> },
    ListOfEntries(Vec<ArchiveEntry>),
}

struct Processor {
    max: u32,
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

                let container = match self.extract_container(&mut file, &format!("{relative_filename}_out"), 0, 0) {
                    Ok(c) => c,
                    Err(e) => {
                        println!("{e}");
                        eprintln!("{relative_filename}: {e}");
                        continue;
                    },
                };
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

    fn extract_container(&self, file: &mut File, output_file: &str, offset: u32, depth: u32) -> std::io::Result<Container> {
        let header = self.read_header(file)?;
        self.display_header(&header, depth);
        let previous_header_size = header.len() * 4;
        let mut previous_header_size_and_offset = previous_header_size as u32 + offset;
        let header_size = header[3];

        let mut header = self.read_header(file)?;
        self.display_header(&header, depth);
        let format = header[2];
        let children_alignment = header[3];
        let mut children = vec![];

        let child_count = header[1];
        if child_count > 0 {
            let expected_size = (child_count * 2 + 5) as usize;

            if header.len() < expected_size {
                let expected_size = (child_count * 2 + 5) as usize;
                while header.len() < expected_size {
                    header.push(file.read_u32::<LittleEndian>()?);
                }
            }

            match header[2] {
                1 | 3 | 4 | 7 => {
                    let child = self.extract_list(file, output_file, previous_header_size_and_offset, depth + 1, &header, header[2])?;
                    children.push(child);
                },
                2 => {
                    let mut vec = vec![];

                    for i in 0..header[1] {
                        let before = file.seek(SeekFrom::Current(0))?;
                        let child = self.extract_block_2(file, output_file, previous_header_size_and_offset, depth + 1)?;
                        vec.push(child);
                        let after = file.seek(SeekFrom::Current(0))?;
                        previous_header_size_and_offset += (after - before) as u32;
                    }

                    children.push(ArchiveEntry::ListOfEntries(vec));
                },
                6 => {
                    let child = self.extract_block(file, output_file, previous_header_size_and_offset, depth + 1, &header, 6)?;
                    children.push(child);
                },
                0 => {
                    for child in 0..child_count {
                        let offset = header[(child + 5) as usize] + previous_header_size_and_offset;
                        let size = header[(child + 5 + child_count) as usize];
                        self.print(depth + 1); println!("{child}: from {:#X} to {:#X}", offset, offset + size);
                        
                        file.seek(SeekFrom::Start((offset) as u64))?;
                        let magic = file.read_u32::<LittleEndian>()?;
                        if magic == 1 {
                            file.seek(SeekFrom::Current(-4))?;
                            let child = self.extract_container(file, output_file, offset, depth + 1)?;
                            children.push(ArchiveEntry::Container(child));
                        } else {
                            self.print(depth + 2); println!("Unknown child format: {:#X}", magic);
                            self.extract_file(file, output_file, offset, size, None)?;
                            children.push(ArchiveEntry::File(format!("{:#X}.bin", offset)));
                        }
                    }
                },
                _ => {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Invalid header type: {:#X}", header[2])));
                },
            };
        }

        Ok(Container {
            format,
            header_size,
            children_alignment,
            children,
        })
    }

    fn extract_list(&self, file: &mut File, output_file: &str, offset: u32, depth: u32, header: &Vec<u32>, id: u32) -> std::io::Result<ArchiveEntry> {
        let child_count = header[1];
        let strings_start = header[5] + offset;
        let strings_size = header[5 + child_count as usize];

        file.seek(SeekFrom::Start(strings_start as u64))?;
        let mut buffer = vec![0u8; strings_size as usize];
        file.read(&mut buffer)?;
        let strings = match String::from_utf8(buffer) {
            Ok(s) => s,
            Err(e) => {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Not a string at {:#X} (size {})", strings_start, strings_size)));
            },
        };
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
                _ => {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Unknown file magic: {:?}", magic)));
                },
            };
            let name_override = format!("{}.{}", filenames[i], extension);
            files.push(name_override.clone());

            if let Some(output_dir) = &self.extract {
                self.extract_file(file, output_file, offset, size, Some(name_override))?;
            }
        }

        Ok(ArchiveEntry::List { id, files })
    }

    fn extract_block_2(&self, file: &mut File, output_file: &str, offset: u32, depth: u32) -> std::io::Result<ArchiveEntry> {
        let header = self.read_header(file)?;
        self.display_header(&header, depth);
        let header = self.read_header(file)?;
        self.display_header(&header, depth);
        let offset = offset + 0x200;

        if header == [0, 0, 0, 0] {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, "Unrecognized data (empty)".to_string()));
        }

        self.extract_block(file, output_file, offset, depth, &header, 2)
    }

    fn extract_block(&self, file: &mut File, output_file: &str, offset: u32, depth: u32, header: &Vec<u32>, id: u32) -> std::io::Result<ArchiveEntry> {
        if header.len() < 5 {
            eprintln!("{output_file}: {:?}", header);
        }

        let strings_start = header[5] + offset;
        let strings_size = header[7];
        let files_start = header[6] + offset;
        let files_size = header[8];

        file.seek(SeekFrom::Start(strings_start as u64))?;
        let mut buffer = vec![0u8; strings_size as usize];
        file.read(&mut buffer)?;

        let strings = String::from_utf8(buffer).unwrap();
        let filenames: Vec<_> = strings.split(",").map(|e| e.trim()).filter(|e| !e.is_empty()).collect();

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
            let mut dds_buffer = vec![0u8; files_size[i] as usize];
            file.read(&mut dds_buffer);
            let save = file.seek(SeekFrom::Current(0))?;
            let dds = Dds::read(&*dds_buffer).unwrap();
            textures.push(Texture {
                name: filenames[i].to_string(),
                format: d3d_to_dds(&dds.get_d3d_format().unwrap()),
                filename: filenames[i].to_string(),
            });
        }

        let save = file.seek(SeekFrom::Current(0))?;
        if let Some(output_dir) = &self.extract {
            std::fs::create_dir_all(format!("{output_dir}/{output_file}"))?;

            let save = file.seek(SeekFrom::Current(0))?;
            for i in 0..filenames.len() {
                file.seek(SeekFrom::Start(files_offset[i] as u64))?;
                let dds = Dds::read(&*file).unwrap();
                let image = image_from_dds(&dds, 0).unwrap();
                let new_filename = format!("{} ({}x{})", filenames[i], image.width(), image.height());
                image.save(&format!("{output_dir}/{output_file}/{}.png", new_filename)).unwrap();
                textures[i].filename = new_filename;
            }
            file.seek(SeekFrom::Start(save))?;
        }

        if save % 256 > 0 {
            let padding = 256 - (save % 256);
            for _ in 0..padding {
                file.read_u8()?;
            }
        }

        Ok(ArchiveEntry::ListOfTextures(textures))
    }

    fn extract_file(&self, file: &mut File, output_file: &str, start: u32, size: u32, name_override: Option<String>) -> std::io::Result<()> {
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

        Ok(header)
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
        let is_root = addr == 0;

        file.write_u32::<LittleEndian>(1)?;
        file.write_u32::<LittleEndian>(1)?;
        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(container.header_size)?;
        let container_size_addr = file.seek(SeekFrom::Current(0))?;
        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(container.format)?;
        file.write_u32::<LittleEndian>(0)?;
        for _ in 0..(container.header_size / 4 - 7) {
            file.write_u32::<LittleEndian>(0)?;
        }

        file.write_u32::<LittleEndian>(0)?;
        let child_count_addr = file.seek(SeekFrom::Current(0))?;
        file.write_u32::<LittleEndian>(0)?;
        file.write_u32::<LittleEndian>(container.format)?;
        file.write_u32::<LittleEndian>(container.children_alignment)?;
        file.write_u32::<LittleEndian>(0)?;
        let children_offsets_addr = file.seek(SeekFrom::Current(0))?;

        let mut real_children_count = 0;
        for child in container.children.iter() {
            real_children_count += match child {
                ArchiveEntry::List { id, files } => 2,
                ArchiveEntry::ListOfTextures(textures) => 1,
                ArchiveEntry::ListOfEntries(entries) => entries.len(),
                ArchiveEntry::Container(_) => 1,
                ArchiveEntry::File(_) => 1,
                _ => panic!("{:?}", child),
            };
        }

        for _ in 0..(real_children_count * 2) {
            file.write_u32::<LittleEndian>(0)?;
        }

        let pos_after_padding = file.seek(SeekFrom::Current(0))?;
        let mut padding = pos_after_padding as u32;
        if padding % container.children_alignment > 0 {
            padding = container.children_alignment - (padding % container.children_alignment);
            for _ in 0..padding {
                file.write_u8(0)?;
            }
        }

        let mut all_children = vec![];
        for (id, child) in container.children.iter().enumerate() {
            all_children.extend(match child {
                ArchiveEntry::List { id, files } => self.pack_list(file, id, files)?,
                ArchiveEntry::ListOfTextures(textures) => self.pack_block(file, textures)?,
                ArchiveEntry::ListOfEntries(entries) => self.pack_list_of_blocks(file, entries)?,
                ArchiveEntry::Container(container) => self.pack_container(file, container)?,
                ArchiveEntry::File(filename) => self.pack_file(file, filename)?,
                _ => panic!("{:?}", child),
            });

            let alignment = container.children_alignment as u64;
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

    fn pack_list(&self, file: &mut File, id: &u32, files: &Vec<String>) -> std::io::Result<Vec<ChildData>> {
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
        for (id, f) in files.iter().enumerate() {
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

        for (i, entry) in entries.iter().enumerate() {
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
        let mut files_offset = vec![data_begin_addr];
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
        file.write_u32::<LittleEndian>(256);
        file.write_u32::<LittleEndian>(256 + (textures_begin_addr - data_begin_addr) as u32);
        file.write_u32::<LittleEndian>(strings.len() as u32);

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
        file.write_u32::<LittleEndian>(full_size);

        file.seek(SeekFrom::Start(data_begin_addr - (512 - 16)))?;
        let saved_pos_mod = 0x100 - (saved_pos % 0x100);
        let cont_size = saved_pos - (data_begin_addr - saved_pos_mod) + 256;
        file.write_u32::<LittleEndian>(cont_size as u32);

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
        DdsFormat::Dxt3 => D3DFormat::DXT1, // image_dds doesn't support DXT3
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
