use crate::ddsfile::*;
use image_dds::*;
use std::path::Path;
use std::fs::File;
use clap::Parser;
use clap_derive::Parser;
use image_dds::ddsfile::Dds;

mod archive;
use archive::*;

mod texture;
use texture::*;

#[derive(Parser, Debug)]
#[command(author = None, version = None, about = None, long_about = None)]
struct Args {
    /// Path to a .cat file to analyse/extract or folder to pack into a .cat file (if --pack is set)
    input: String,

    /// Path to a folder where to extract the .cat file
    #[arg(short, long, conflicts_with="pack")]
    extract: Option<String>,

    /// Path to a directory that will be packed in a .cat file
    #[arg(short, long, conflicts_with="extract")]
    pack: Option<String>,
}

fn main() {
    let args = Args::parse();

    let path = Path::new(&args.input);
    if !path.exists() {
        eprintln!("'{}' does not exist.", args.input);
        return;
    }

    if let Some(output) = args.pack {
        if !path.is_dir() {
            eprintln!("'{}' is not a directory.", args.input);
            return;
        }

        let mut writer = CatFileWriter::new(args.input, &output);
        if let Err(e) = writer.pack() {
            eprintln!("{e}");
        } else {
            println!("OK");
        }
    } else {
        if !path.is_file() {
            eprintln!("'{}' is not a file.", args.input);
            return;
        }

        let mut reader = CatFileReader::new(&args.input, args.extract.as_deref().map(str::to_string));
        match reader.unpack() {
            Ok(obj) => {
                if let Some(output) = args.extract {
                    std::fs::create_dir_all(format!("{output}")).unwrap();
                    let writer = File::create(format!("{output}/metadata.json")).unwrap();
                    serde_json::to_writer_pretty(writer, &obj).unwrap();
                }
                println!("OK");
            },
            Err(e) => {
                eprintln!("{e}");
            },
        };
    }
}
