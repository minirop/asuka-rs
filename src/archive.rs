use crate::texture::HeaderConverter;
use crate::texture;
use image_dds::image::buffer::ConvertBuffer;
use image_dds::image::io::Reader as ImageReader;
use crate::image::*;
use std::fs::File;
use image_dds::image_from_dds;
use crate::Dds;
use byteorder::*;
use std::io::*;
use crate::Texture;
use serde::*;

#[derive(Debug)]
pub struct ChildData {
    pub offset: u64,
    pub size: u64,
}

#[derive(Debug)]
pub struct ContainerHeader {
    pub version: u32,
    pub format: u32,
    pub size: u32,
    pub content_size: u32,
    pub alignment: u32,
    pub children: Vec<ChildData>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Container {
    pub version: u32,
    pub format: u32,
    pub size: u32,
    pub alignment: u32,
    pub children: Vec<ArchiveEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ArchiveEntry {
    Container(Container),
    Textures(Vec<Texture>),
    File(String),
    Files(Vec<String>),
}

pub trait SeekRead: Read + Seek {}
impl SeekRead for std::fs::File {}

pub struct CatFileReader {
    pub input: File,
    pub output: Option<String>,
}

impl CatFileReader {
    pub fn new(input: &str, output: Option<String>) -> Self {
        Self {
            input: File::open(&input).unwrap(),
            output,
        }
    }

    pub fn unpack(&mut self) -> std::io::Result<ArchiveEntry> {
        let value = self.peek_u32();
        if value == 1 {
            self.unpack_container()
        } else {
            self.unpack_gxt()
        }
    }

    fn unpack_container(&mut self) -> std::io::Result<ArchiveEntry> {
        let container_end = self.get_offset();

        let Ok(header) = self.read_header() else {
            let pos = self.get_offset();
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Invalid header at position {:#X}", pos)));
        };

        let container_end = container_end + (header.size + header.content_size) as u64;

        assert!(header.version < 3);

        let mut children = vec![];

        match header.format {
            0 => match (header.size, header.alignment) {
                (64, 64) => {
                    for child in &header.children {
                        self.input.seek(SeekFrom::Start(child.offset))?;
                        let val = self.peek_u32();
                        assert_eq!(val, 0);

                        children.push(ArchiveEntry::File(format!("{:#X}.bin", child.size)));
                    }
                },
                _ => {
                    for (id, child) in header.children.iter().enumerate() {
                        self.input.seek(SeekFrom::Start(child.offset))?;
                        let val = self.peek_u32();
                        if val == 1 {
                            children.push(self.unpack_container()?);
                        } else {
                            println!("{id}: {:#X} [{}, {}, {}, {}]", child.offset,
                                self.input.read_u32::<LittleEndian>()?,
                                self.input.read_u32::<LittleEndian>()?,
                                self.input.read_u32::<LittleEndian>()?,
                                self.input.read_u32::<LittleEndian>()?);
                        }
                    }
                }
            },
            1 => {
                children.push(self.unpack_format_1(&header)?);
            },
            2 => {
                for child in header.children.iter() {
                    children.push(self.unpack_format_2(child)?);
                }
            },
            5 => {
                for (id, child) in header.children.iter().enumerate() {
                    if let Some(output_dir) = &self.output {
                        std::fs::create_dir_all(output_dir)?;
                        self.extract_file(child, &format!("{output_dir}/child-{id}.bin"))?;
                    } else {
                        println!("{id}: {:?}", child);
                    }
                }
            },
            6 =>{
                assert_eq!(header.children.len(), 2);

                children.push(self.unpack_format_6()?);
            },
            8 => {
                assert_eq!(header.children.len(), 1);

                children.push(self.unpack_format_8()?);
            },
            _ => {
                println!("Unknown format {}.", header.format);
                for (id, child) in header.children.iter().enumerate() {
                    self.input.seek(SeekFrom::Start(child.offset))?;
                    println!("{id}: {:#X} [{}, {}, {}, {}]", child.offset,
                        self.input.read_u32::<LittleEndian>()?,
                        self.input.read_u32::<LittleEndian>()?,
                        self.input.read_u32::<LittleEndian>()?,
                        self.input.read_u32::<LittleEndian>()?);
                }
            }
        };

        self.input.seek(SeekFrom::Start(container_end))?;

        Ok(ArchiveEntry::Container(Container {
            version: header.version,
            format: header.format,
            size: header.size,
            alignment: header.alignment,
            children,
        }))
    }

    fn unpack_gxt(&mut self) -> std::io::Result<ArchiveEntry> {
        let images_data = self.unpack_block(0)?;
        let mut textures = vec![];

        for (id, image_data) in images_data.iter().enumerate() {
            let mut buffer = vec![0u8; image_data.size as usize];
            self.input.read(&mut buffer)?;
            let dds = Dds::read(&*buffer).unwrap();
            let image = image_from_dds(&dds, 0).unwrap();
            let filename = format!("{:#X}.png", image_data.offset);
            let format = dds.get_texture_format();

            if let Some(output_dir) = &self.output {
                std::fs::create_dir_all(output_dir)?;
                image.save(&format!("{output_dir}/{filename}")).unwrap();
            } else {
                println!("{id}: {filename}");
            }

            textures.push(Texture {
                name: "".into(),
                format,
                filename,
            });
        }

        Ok(ArchiveEntry::Textures(textures))
    }

    fn unpack_format_1(&mut self, header: &ContainerHeader) -> std::io::Result<ArchiveEntry> {
        let strings = &header.children[0];
        let strings = self.read_strings(strings.offset, strings.size);

        let mut files = vec![];
        for (id, child) in header.children.iter().skip(1).enumerate() {
            let filename = strings[id].clone();

            if let Some(output_dir) = &self.output {
                std::fs::create_dir_all(output_dir)?;
                self.extract_file(child, &format!("{output_dir}/{filename}"))?;
            } else {
                println!("{id}: {filename}");
            }

            files.push(filename);
        }

        Ok(ArchiveEntry::Files(files))
    }

    fn unpack_format_2(&mut self, data: &ChildData) -> std::io::Result<ArchiveEntry> {
        self.input.seek(SeekFrom::Start(data.offset))?;

        let Ok(header) = self.read_header() else {
            let pos = self.get_offset();
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Invalid header at position {:#X}", pos)));
        };

        assert_eq!(header.format, 0);
        assert_eq!(header.children.len(), 2);

        let strings = &header.children[0];
        let images = &header.children[1];

        let strings = self.read_strings(strings.offset, strings.size);

        self.input.seek(SeekFrom::Start(images.offset))?;
        let magic = self.peek_u32_be();

        let mut textures = vec![];
        match magic {
            GNF => {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("GNF Textures are not supported")));
            },
            _ => {
                let images_data = self.unpack_block(images.offset)?;

                for (id, name) in strings.iter().enumerate() {
                    let image = &images_data[id];
                    let mut buffer = vec![0u8; image.size as usize];
                    self.input.read(&mut buffer)?;
                    let dds = Dds::read(&*buffer).unwrap();
                    let image = image_from_dds(&dds, 0).unwrap();
                    let filename = format!("{name} ({}x{}).png", image.width(), image.height());
                    let format = dds.get_texture_format();

                    if let Some(output_dir) = &self.output {
                        std::fs::create_dir_all(output_dir)?;
                        image.save(&format!("{output_dir}/{filename}")).unwrap();
                    } else {
                        println!("{id}: {filename}");
                    }

                    textures.push(Texture {
                        name: name.clone(),
                        format,
                        filename,
                    });
                }
            }
        };

        Ok(ArchiveEntry::Textures(textures))
    }

    fn unpack_format_6(&self) -> std::io::Result<ArchiveEntry> {
        todo!();
        //Ok(ArchiveEntry::Textures(vec![]))
    }

    fn unpack_format_8(&mut self) -> std::io::Result<ArchiveEntry> {
        let Ok(header) = self.read_header() else {
            let pos = self.get_offset();
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Invalid header at position {:#X}", pos)));
        };

        assert_eq!(header.format, 0);
        assert_eq!(header.children.len(), 2);

        let strings = &header.children[0];
        let files = &header.children[1];

        //let strings = self.read_strings(strings.offset, strings.size);
        let mut children = vec![];

        if let Some(output_dir) = &self.output.clone() {
            std::fs::create_dir_all(output_dir)?;
            self.extract_file(strings, &format!("{output_dir}/{:#X}.bin", strings.offset))?;
            self.extract_file(files, &format!("{output_dir}/{:#X}.bin", files.offset))?;

            children.push(format!("{:#X}.bin", strings.offset));
            children.push(format!("{:#X}.bin", files.offset));
        } else {
            println!("0: Unknown format");
        }

        Ok(ArchiveEntry::Files(children))
    }

    fn unpack_block(&mut self, offset: u64) -> std::io::Result<Vec<ChildData>> {
        let mut children_data: Vec<ChildData> = vec![];

        let mut block_start = self.input.seek(SeekFrom::Start(offset))?;

        // arbitrary size
        if self.peek_u32() > 0xFF {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Block without a header is not supported, at pos {:#X}.", self.get_offset())));
        }

        let block_header_size = self.input.read_u32::<LittleEndian>()?;
        let children_count = self.input.read_u32::<LittleEndian>()?;
        let block_size = self.input.read_u32::<LittleEndian>()?;
        block_start += block_header_size as u64;
        for _ in 0..children_count {
            children_data.push(ChildData {
                offset: self.input.read_u32::<LittleEndian>()? as u64 + block_start,
                size: 0,
            });
        }

        for i in 0..(children_count - 1) {
            let k = i as usize;
            children_data[k].size = children_data[k + 1].offset - children_data[k].offset;
        }
        let last_child = children_data.last_mut().unwrap();
        last_child.size = (block_start + block_size as u64) - last_child.offset;

        Ok(children_data)
    }

    fn read_header(&mut self) -> std::io::Result<ContainerHeader> {
        let mut children = vec![];
        
        // part one
        let val = self.input.read_u32::<LittleEndian>()?; assert_eq!(val, 1);
        let version = self.input.read_u32::<LittleEndian>()?;
        let val = self.input.read_u32::<LittleEndian>()?; assert_eq!(val, 0);
        let mut size = self.input.read_u32::<LittleEndian>()?;
        if size == 0 {
            println!("[DEBUG] read_header: size is 0.");
            size = 256;
        }
        assert!(size >= 32);

        let content_size = self.input.read_u32::<LittleEndian>()?;

        // maybe check the duplicate values?
        let byte_zero = self.input.seek(SeekFrom::Current(size as i64 - 20))?;

        // part two
        let val = self.input.read_u32::<LittleEndian>()?; assert_eq!(val, 0);
        let child_count = self.input.read_u32::<LittleEndian>()? as usize;
        let format = self.input.read_u32::<LittleEndian>()?;
        let alignment = self.input.read_u32::<LittleEndian>()?;
        let val = self.input.read_u32::<LittleEndian>()?; assert_eq!(val, 0);

        let mut data = vec![];
        for _ in 0..child_count {
            data.push(self.input.read_u32::<LittleEndian>()?);
            data.push(self.input.read_u32::<LittleEndian>()?);
        }

        self.align(alignment);

        for i in 0..(child_count) {
            let offset = data[i] as u64 + byte_zero;
            let size = data[i + child_count] as u64;
            children.push(ChildData {
                offset, size,
            });
        }

        let header = ContainerHeader {
            version,
            format,
            size,
            content_size,
            alignment,
            children,
        };
        if self.output.is_none() {
            println!("{:?}", header);
        }

        Ok(header)
    }

    fn get_offset(&mut self) -> u64 {
        self.input.seek(SeekFrom::Current(0)).unwrap()
    }

    fn align(&mut self, alignment: u32) {
        if alignment > 0 {
            let alignment = alignment as u64;
            let cur_pos = self.input.seek(SeekFrom::Current(0)).unwrap();
            if (cur_pos % alignment) != 0 {
                let cur_pos = (alignment - (cur_pos % alignment)) as i64;
                self.input.seek(SeekFrom::Current(cur_pos)).unwrap();
            }
        } else {
            println!("[DEBUG] Alignement is NULL");
        }
    }

    fn peek_u32(&mut self) -> u32 {
        let val = self.input.read_u32::<LittleEndian>().unwrap();
        self.input.seek(SeekFrom::Current(-4)).unwrap();
        val
    }

    fn peek_u32_be(&mut self) -> u32 {
        let val = self.input.read_u32::<BigEndian>().unwrap();
        self.input.seek(SeekFrom::Current(-4)).unwrap();
        val
    }

    fn extract_file(&mut self, data: &ChildData, filename: &str) -> std::io::Result<()> {
        self.input.seek(SeekFrom::Start(data.offset)).unwrap();
        let mut buffer = vec![0u8; data.size as usize];
        self.input.read(&mut buffer)?;

        std::fs::write(filename, buffer)?;

        Ok(())
    }

    fn read_strings(&mut self, offset: u64, size: u64) -> Vec<String> {
        self.input.seek(SeekFrom::Start(offset)).unwrap();
        let mut buffer = vec![0u8; size as usize];
        self.input.read(&mut buffer).unwrap();
        let Ok(strings) = String::from_utf8(buffer.clone()) else {
            eprintln!("Not a string buffer at offset {:#X}", offset);
            return vec![];
        };
        let filenames: Vec<_> = strings.split("\n").map(|e| e.trim().trim_end_matches(',').to_string()).filter(|e| !e.is_empty()).collect();

        filenames
    }
}

pub trait SeekWrite: Write + Seek {}
impl SeekWrite for std::fs::File {}

pub struct CatFileWriter {
    pub input: ArchiveEntry,
    pub output: File,
    root: String,
}

impl CatFileWriter {
    pub fn new(input: String, output: &str) -> Self {
        let root = if input.ends_with("/") {
            input.clone()
        } else {
            format!("{}/", input)
        };

        let input = if input.ends_with("/") {
            format!("{}metadata.json", input)
        } else {
            format!("{}/metadata.json", input)
        };

        let strbuf = std::fs::read_to_string(input).unwrap();
        let entry: ArchiveEntry = serde_json::from_str(&strbuf).unwrap();

        Self {
            input: entry,
            output: File::create(&output).unwrap(),
            root,
        }
    }

    pub fn pack(&mut self) -> std::io::Result<()> {
        let entry = self.input.clone();
        match &entry {
            ArchiveEntry::Container(container) => self.pack_container(container),
            _ => panic!("Unsupported entry: {:?}", entry),
        }
    }

    fn pack_container(&mut self, container: &Container) -> std::io::Result<()> {
        let start_of_container = self.get_offset();
        self.write_header(container.version, container.format, container.size, container.alignment, container.children.len())?;

        let start_of_children_offsets = start_of_container + container.size as u64 + 20;
        let start_of_children_sizes = start_of_children_offsets + container.children.len() as u64 * 4;

        match container.format {
            0 => {
                for (id, child) in container.children.iter().enumerate() {
                    let child_start = self.get_offset();
                    let relative_child_start = (child_start - start_of_container) as u32 - container.size;
                    self.write_at(start_of_children_offsets + id as u64 * 4, relative_child_start);
                    match child {
                        ArchiveEntry::Container(container) => self.pack_container(container)?,
                        _ => panic!("Unsupported child: {:?}", child),
                    }

                    self.align(container.alignment);

                    let child_size = self.get_offset() - child_start;
                    self.write_at(start_of_children_sizes + id as u64 * 4, child_size as u32);
                }
            },
            1 => {
                self.pack_format_1(start_of_container, &container)?;
            },
            2 => {
                let child_start = self.get_offset();
                let relative_child_start = (child_start - start_of_container) as u32 - container.size;
                self.write_at(start_of_children_offsets, relative_child_start);

                self.pack_format_2(container)?;

                let child_size = self.get_offset() - child_start;
                self.write_at(start_of_children_sizes, child_size as u32);
            },
            8 => {
                let child_start = self.get_offset();
                let relative_child_start = (child_start - start_of_container) as u32 - container.size;
                self.write_at(start_of_children_offsets, relative_child_start);

                self.pack_format_8(&container)?;

                let child_size = self.get_offset() - child_start;
                self.write_at(start_of_children_sizes, child_size as u32);
            },
            _ => panic!("Unhandled format: {}", container.format),
        };

        let end_of_container = self.get_offset();
        let container_size = (end_of_container - start_of_container) as u32 - container.size;
        self.write_at(start_of_container + 16, container_size);

        Ok(())
    }

    fn pack_format_1(&mut self, start_of_container: u64, container: &Container) -> std::io::Result<()> {
        assert_eq!(container.children.len(), 1);
        self.write_at(start_of_container + 0x104, 2);

        let start_of_children_offsets = start_of_container + container.size as u64 + 20;

        let child = container.children.first().unwrap();
        match child {
            ArchiveEntry::Files(files) => {
                let mut children_data = vec![];

                if container.format == 1 {
                    let names = files.join(",\r\n");
                    let names = format!("{names},\r\n");
                    let relative_start_of_names = self.get_offset() - start_of_container - 256;

                    write!(self.output, "{}", names)?;
                    self.align(container.alignment);

                    children_data.push(ChildData {
                        offset: relative_start_of_names,
                        size: names.len() as u64,
                    });
                    self.align(container.alignment);
                }

                for file in files {
                    let filename = format!("{}{}", self.root, file);
                    let bin_file = std::fs::read(filename)?;
                    let relative_start_of_file = self.get_offset() - start_of_container - 256;
                    
                    self.output.write(&bin_file)?;
                    self.align(container.alignment);

                    children_data.push(ChildData {
                        offset: relative_start_of_file,
                        size: bin_file.len() as u64,
                    });
                    self.align(container.alignment);
                }

                self.updade_children_offsets_and_sizes(start_of_children_offsets, children_data)?;
            },
            _ => panic!("Unsupported child: {:?}", child),
        };

        Ok(())
    }

    fn pack_format_2(&mut self, container: &Container) -> std::io::Result<()> {
        for child in container.children.iter() {
            match child {
                ArchiveEntry::Textures(textures) => {
                    let start_of_container = self.get_offset();
                    let start_of_children_offsets = start_of_container + 256 + 20;
                    let mut children_data = vec![];

                    self.write_header(1, 0, 256, 256, 2)?;

                    let names = textures.iter().map(|tex| tex.name.clone()).collect::<Vec<_>>().join(",\r\n");
                    let names = format!("{names},\r\n");
                    
                    let relative_start_of_names = self.get_offset() - start_of_container - 256;
                    children_data.push(ChildData {
                        offset: relative_start_of_names,
                        size: names.len() as u64,
                    });

                    write!(self.output, "{}", names)?;
                    self.align(container.alignment);

                    let start_of_image_block = self.get_offset();

                    let tex_count = textures.len() as u32;
                    self.output.write_u32::<LittleEndian>(12 + 4 * tex_count)?;
                    self.output.write_u32::<LittleEndian>(tex_count)?;
                    self.output.write_u32::<LittleEndian>(0x42424242)?;
                    for _ in 0..textures.len() {
                        self.output.write_u32::<LittleEndian>(0x0)?;
                    }

                    for texture in textures {
                        let filename = format!("{}{}", self.root, texture.filename);
                        let img = ImageReader::open(&filename).unwrap().decode().unwrap();
                        let img = match img {
                            DynamicImage::ImageRgba8(image) => image,
                            DynamicImage::ImageRgb8(image) => {
                                let rgba_image: RgbaImage = image.convert();
                                rgba_image
                            },
                            _ => panic!("{} is not a RGB(A) image: {:?}", filename, img),
                        };

                        let dds = texture::dds_from_image(&img, &texture.format).unwrap();
                        dds.write(&mut self.output).unwrap();
                    }
                    
                    let size_of_content_block = self.get_offset() - start_of_image_block;
                    self.write_at(start_of_image_block + 8, size_of_content_block as u32);

                    self.align(container.alignment);

                    let relative_start_of_image_block = start_of_image_block - start_of_container - 256;
                    children_data.push(ChildData {
                        offset: relative_start_of_image_block,
                        size: size_of_content_block,
                    });

                    let container_size = (self.get_offset() - start_of_container) as u32 - container.size;
                    self.write_at(start_of_container + 16, container_size);

                    self.updade_children_offsets_and_sizes(start_of_children_offsets, children_data)?;
                },
                _ => panic!("Unsupported child: {:?}", child),
            };
        }

        Ok(())
    }

    fn pack_format_8(&mut self, container: &Container) -> std::io::Result<()> {
        assert_eq!(container.children.len(), 1);

        let child = container.children.first().unwrap();
        match child {
            ArchiveEntry::Files(files) => {
                assert_eq!(files.len(), 2);

                let start_of_container = self.get_offset();
                let start_of_children_offsets = start_of_container + 256 + 20;

                self.write_header(1, 0, 256, 256, 2)?;

                let mut children_data = vec![];

                if container.format == 1 {
                    let names = files.join(",\r\n");
                    let names = format!("{names},\r\n");
                    let relative_start_of_names = self.get_offset() - start_of_container - 256;

                    write!(self.output, "{}", names)?;
                    self.align(container.alignment);

                    children_data.push(ChildData {
                        offset: relative_start_of_names,
                        size: names.len() as u64,
                    });
                    self.align(container.alignment);
                }

                for file in files {
                    let filename = format!("{}{}", self.root, file);
                    let bin_file = std::fs::read(filename)?;
                    let relative_start_of_file = self.get_offset() - start_of_container - 256;
                    
                    self.output.write(&bin_file)?;
                    self.align(container.alignment);

                    children_data.push(ChildData {
                        offset: relative_start_of_file,
                        size: bin_file.len() as u64,
                    });
                    self.align(container.alignment);
                }

                let container_size = (self.get_offset() - start_of_container) as u32 - container.size;
                self.write_at(start_of_container + 16, container_size);

                self.updade_children_offsets_and_sizes(start_of_children_offsets, children_data)?;
            },
            _ => panic!("Unsupported child: {:?}", child),
        };
        Ok(())
    }

    fn write_header(&mut self, version: u32, format: u32, size: u32, alignment: u32, children_count: usize) -> std::io::Result<()> {
        self.output.write_u32::<LittleEndian>(1)?;
        self.output.write_u32::<LittleEndian>(version)?;
        self.output.write_u32::<LittleEndian>(0)?;
        self.output.write_u32::<LittleEndian>(size)?;
        self.output.write_u32::<LittleEndian>(0)?;
        self.output.write_u32::<LittleEndian>(format)?;
        self.output.write_u32::<LittleEndian>(children_count as u32)?;
        let size = if size == 0 { 256 } else { size };
        for _ in 0..((size / 4) - 6) {
            self.output.write_u32::<LittleEndian>(0)?;
        }
        self.output.write_u32::<LittleEndian>(children_count as u32)?;
        self.output.write_u32::<LittleEndian>(format)?;
        self.output.write_u32::<LittleEndian>(alignment)?;
        self.output.write_u32::<LittleEndian>(0)?;
        for _ in 0..children_count {
            self.output.write_u32::<LittleEndian>(0)?;
            self.output.write_u32::<LittleEndian>(0)?;
        }
        self.align(alignment);

        Ok(())
    }

    fn get_offset(&mut self) -> u64 {
        self.output.seek(SeekFrom::Current(0)).unwrap()
    }

    fn write_at(&mut self, pos: u64, value: u32) {
        let orig = self.get_offset();
        self.goto(pos);
        self.output.write_u32::<LittleEndian>(value).unwrap();
        self.goto(orig);
    }

    fn goto(&mut self, pos: u64) {
        self.output.seek(SeekFrom::Start(pos)).unwrap();
    }

    fn updade_children_offsets_and_sizes(&mut self, pos: u64, children: Vec<ChildData>) -> std::io::Result<()> {
        let orig = self.get_offset();
        self.goto(pos);

        for c in &children {
            self.output.write_u32::<LittleEndian>(c.offset as u32).unwrap();
        }
        for c in &children {
            self.output.write_u32::<LittleEndian>(c.size as u32).unwrap();
        }

        self.goto(orig);
        Ok(())
    }

    fn align(&mut self, alignment: u32) {
        if alignment > 0 {
            let alignment = alignment as u64;
            let cur_pos = self.output.seek(SeekFrom::Current(0)).unwrap();
            if (cur_pos % alignment) != 0 {
                let cur_pos = (alignment - (cur_pos % alignment)) as i64;
                for _ in 0..cur_pos {
                    self.output.write_u8(0).unwrap();
                }
            }
        } else {
            println!("[DEBUG] Alignement is NULL");
        }
    }
}

#[allow(unused)]
pub const A001: u32 = 0x61303031u32;
pub const GNF : u32 = 0x474E4620u32;
#[allow(unused)]
pub const TMD0: u32 = 0x746D6430u32;
#[allow(unused)]
pub const TMO1: u32 = 0x746D6F31u32;
