use std::{env, thread};
use std::fs::File;
use std::io::{BufReader, Read, BufRead, SeekFrom, Seek, Write};
use std::process::exit;
use std::convert::TryFrom;
use std::path::Path;
use std::io::SeekFrom::Current;
use indicatif::{ProgressStyle, MultiProgress, ProgressBar, HumanBytes};
use clap::Clap;

fn main() {
    println!("Danganronpa WAD Extractor");

    let args: Opts = Opts::parse();
    let file = &args.wad_file;

    println!("Loading {}...", Path::new(&file).file_stem().unwrap().to_string_lossy());
    let mut buf = BufReader::new(File::open(file).expect("Cannot open file!"));
    buf.fill_buf().unwrap();

    // Check our header
    let header = &buf.buffer()[..4];
    let magic = String::from_utf8_lossy(header).into_owned();
    buf.seek(SeekFrom::Current(4)).unwrap();

    if !(magic == "AGAR") {
        println!("❌ Not a supported file (Cannot find magic bytes)!");
        exit(1);
    }

    // Skip the rest of the header
    buf.seek(SeekFrom::Current(12)).unwrap();
    buf.fill_buf().unwrap();

    // Get our file count
    let mut dst = [0u8; 4];
    dst.copy_from_slice(&buf.buffer()[..4]);
    let file_count = i32::from_le_bytes(dst);
    buf.seek(SeekFrom::Current(4)).unwrap();

    // Prepare our fileinfos
    let mut file_infos: Vec<FileInfo> = Vec::new();

    // iterate over our filecount
    for _ in 0..file_count {
        buf.fill_buf().unwrap();

        // Get the length of the name
        let mut dst = [0u8; 4];
        dst.copy_from_slice(&buf.buffer()[..4]);
        let str_len = i32::from_le_bytes(dst);
        buf.seek(SeekFrom::Current(4)).unwrap();

        // Get the name
        buf.fill_buf().unwrap();
        let file_name = String::from_utf8_lossy(&buf.buffer()[..str_len as usize]).into_owned();
        buf.seek(SeekFrom::Current(str_len as i64)).unwrap();

        // Get the length of the file in bytes
        buf.fill_buf().unwrap();
        dst.copy_from_slice(&buf.buffer()[..4]);
        let file_len = i32::from_le_bytes(dst);
        buf.seek(SeekFrom::Current(4)).unwrap();

        // Skip unknown variable
        buf.seek(SeekFrom::Current(4)).unwrap();

        // Get the offset for later
        buf.fill_buf().unwrap();
        dst.copy_from_slice(&buf.buffer()[..4]);
        let offset = i32::from_le_bytes(dst);
        buf.seek(SeekFrom::Current(4)).unwrap();

        // Skip unknown variable
        buf.seek(SeekFrom::Current(4)).unwrap();

        // Put it in our list.
        file_infos.push(FileInfo {
            filename: file_name,
            length: file_len,
            offset: offset,
        });
    }

    // Get the folder count.
    buf.fill_buf().unwrap();
    dst.copy_from_slice(&buf.buffer()[..4]);
    let folder_count = i32::from_le_bytes(dst);
    buf.seek(SeekFrom::Current(4)).unwrap();

    println!("Got {} 📝 files in {} 📁 folders", file_count, folder_count);

    // Prepare our folderinfos
    let mut folder_infos: Vec<FolderInfo> = Vec::new();

    // iterate over our foldercount
    for _ in 0..folder_count {
        buf.fill_buf().unwrap();
        dst.copy_from_slice(&buf.buffer()[..4]);
        let str_len = i32::from_le_bytes(dst);
        buf.seek(SeekFrom::Current(4)).unwrap();

        let mut folder_name = "".to_string();

        // Exception for root folder, which has a 0 string length
        if str_len != 0 {
            buf.fill_buf().unwrap();
            dst.copy_from_slice(&buf.buffer()[..4]);
            folder_name = String::from_utf8_lossy(&buf.buffer()[..str_len as usize]).into_owned();
            buf.seek(SeekFrom::Current(str_len as i64)).unwrap();
        }

        // Get the object count in the folder.
        // Could be a directory of a file.
        buf.fill_buf().unwrap();
        dst.copy_from_slice(&buf.buffer()[..4]);
        let files_in_folder = i32::from_le_bytes(dst);
        buf.seek(SeekFrom::Current(4)).unwrap();

        // Prepare our folderfileinfos.
        let mut folder_file_infos: Vec<FolderFileInfo> = Vec::new();

        for _ in 0..files_in_folder {
            // Get the string length.
            buf.fill_buf().unwrap();
            dst.copy_from_slice(&buf.buffer()[..4]);
            let str_len = i32::from_le_bytes(dst);
            buf.seek(SeekFrom::Current(4)).unwrap();

            // Get the filename.
            buf.fill_buf().unwrap();
            dst.copy_from_slice(&buf.buffer()[..4]);
            let file_name =  String::from_utf8_lossy(&buf.buffer()[..str_len as usize]).into_owned();
            buf.seek(SeekFrom::Current(str_len as i64)).unwrap();

            // Get the file type.
            buf.fill_buf().unwrap();
            let file_type_byte = &buf.buffer()[0];
            let file_type: FileType = FileType::try_from(*file_type_byte).unwrap();
            buf.seek(SeekFrom::Current(1)).unwrap();

            // Add it to our folderfileinfos.
            folder_file_infos.push(FolderFileInfo {
                filename: file_name,
                filetype: file_type
            })
        }

        // Add everything to our folderinfos.
        folder_infos.push(FolderInfo {
            filename: folder_name,
            filesinfolder: files_in_folder,
            folderfileinfos: folder_file_infos
        })
    }

    // offset 0 of data
    let base = buf.seek(SeekFrom::Current(0)).unwrap();

    if args.extract_location.is_some() {
        println!("Extracting to {}", Path::new(args.extract_location.as_ref().unwrap()).join(Path::new(&file).file_stem().unwrap()).to_string_lossy());
    } else {
        println!("Extracting to {}", env::current_dir().unwrap().join(Path::new(&file).file_stem().unwrap()).to_string_lossy());
    }

    // Set up our progress bars.
    let multi_prog = MultiProgress::new();

    let total_size_progress_bar = multi_prog.add(ProgressBar::new(0));
    total_size_progress_bar.set_style(ProgressStyle::default_bar()
        .template("{spinner} [{elapsed_precise}] Extracted {bytes}")
        .progress_chars("##-"));

    let folders_progress_bar = multi_prog.add(ProgressBar::new(folder_count as u64));
    folders_progress_bar.set_style(ProgressStyle::default_bar()
        .template("{bar:40.cyan/blue} {pos:>7}/{len:7} {msg:12}")
        .progress_chars("##-"));

    let file_progress_bar = multi_prog.add(ProgressBar::new(file_count as u64));
    file_progress_bar.set_style(ProgressStyle::default_bar()
        .template("{bar:40.cyan/blue} {pos:>7}/{len:7} ETA {eta} | {prefix:8} {msg:12} ")
        .progress_chars("##-"));

    #[cfg(debug_assertions)]
    let debug_bar = multi_prog.add(ProgressBar::new(0));

    #[cfg(debug_assertions)]
    debug_bar.set_style(ProgressStyle::default_bar().template("SP: {msg}"));

    let progbarthread = thread::spawn(move || { multi_prog.join().unwrap();});

    for folder in folder_infos {
        let extract_to;

        if args.extract_location.is_some() {
            extract_to = Path::new(args.extract_location.as_ref().unwrap()).join(Path::new(&file).file_stem().unwrap())
        } else {
            extract_to = env::current_dir().unwrap().join(Path::new(&file).file_stem().unwrap());
        }
        let current_dir = extract_to.join(Path::new(&folder.filename));
        std::fs::create_dir_all(&current_dir).expect(&format!("Cannot create folder {}. Please run as administrator and try again.\n", &current_dir.to_string_lossy()));

        folders_progress_bar.set_message(&format!("{}", &folder.filename));
        folders_progress_bar.inc(1);
        folders_progress_bar.tick();

        for file in folder.folderfileinfos {
            buf.seek(SeekFrom::Start(base)).unwrap();

            if file.filetype == FileType::FOLDER {
                let path = &current_dir.join(Path::new(&file.filename));
                std::fs::create_dir_all(path).expect(&format!("Cannot create folder {}. Please run as administrator and try again.\n", path.to_string_lossy()));
            }

            let mut new_file = File::create(&current_dir.join(Path::new(&file.filename))).unwrap();
            let mut file_info = Option::None;

            let current_filename = String::from(Path::new(&folder.filename).join(&file.filename).to_str().unwrap());

            for fileinfo in &file_infos {
                if &fileinfo.filename == &current_filename.replace("\\", "/") {
                    file_info = Some(fileinfo);
                    break;
                }
            }

            let mut data = vec![0u8; file_info.unwrap().length as usize].into_boxed_slice();

            if file_info.is_none() {
                println!("Attempted to fetch item {0} but was not in index!", &file.filename);
                file_progress_bar.inc(1);
                continue;
            }

            file_progress_bar.set_message(&format!("{}", &file.filename));
            file_progress_bar.set_prefix(&format!("{}", HumanBytes(data.len() as u64)));

            buf.seek(SeekFrom::Current(file_info.unwrap().offset as i64)).unwrap();
            buf.read_exact(&mut data).unwrap();
            new_file.write_all(&data).unwrap();
            //println!("Write: {0} | Expect: {2} | To: {1} | SP: {3}", data.len(), &file.filename, &file_info.unwrap().length, buf.seek(Current(0)).unwrap());

            #[cfg(debug_assertions)]
            debug_bar.set_message(&format!("{}", buf.seek(Current(0)).unwrap()));

            #[cfg(debug_assertions)]
            debug_bar.tick();
            buf.seek(SeekFrom::Start(base)).unwrap();

            file_progress_bar.inc(1);
            total_size_progress_bar.inc(data.len() as u64);
            file_progress_bar.tick();
            folders_progress_bar.tick();
            total_size_progress_bar.tick();
        }
    }

    folders_progress_bar.finish_with_message("All done!");
    file_progress_bar.set_prefix("");
    file_progress_bar.finish_with_message("All done!");
    total_size_progress_bar.finish_at_current_pos();

    #[cfg(debug_assertions)]
    debug_bar.finish_at_current_pos();

    progbarthread.join().unwrap();
    return
}

#[derive(Clap)]
#[clap(version = "1.0", author = "breadbyte")]
struct Opts {
    /// The WAD File to extract.
    #[clap(long)]
    wad_file: String,

    /// The path to extract to. Optional.
    #[clap(long)]
    extract_location: Option<String>
}

struct FileInfo {
    filename: String,
    length: i32,
    offset: i32,
}

struct FolderInfo {
    filename: String,
    filesinfolder: i32,
    folderfileinfos: Vec<FolderFileInfo>
}

struct FolderFileInfo {
    filename: String,
    filetype: FileType
}

#[derive(PartialEq)]
enum FileType {
    FILE = 0,
    FOLDER = 1
}

impl TryFrom<u8> for FileType {
    type Error = ();

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            x if x == FileType::FILE as u8 => Ok(FileType::FILE),
            x if x == FileType::FOLDER as u8 => Ok(FileType::FOLDER),
            _ => Err(()),
        }
    }
}