use std::cmp::{max, min};
use clap::{crate_version, App, Arg};
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use memmap::{Mmap, MmapOptions};
use libc::ENOENT;
use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};
use std::io::{Result, Error, Read, Seek};
use std::fs::File;
use std::os::unix::fs::FileExt;

const MAX: i32 = 4 * 1024 *1024;
const TTL: Duration = Duration::from_secs(1); // 1 second

static ATTRS: [FileAttr; 2] = [
    FileAttr {
        ino: 1,
        size: 0,
        blocks: 0,
        atime: UNIX_EPOCH, // 1970-01-01 00:00:00
        mtime: UNIX_EPOCH,
        ctime: UNIX_EPOCH,
        crtime: UNIX_EPOCH,
        kind: FileType::Directory,
        perm: 0o755,
        nlink: 2,
        uid: 501,
        gid: 20,
        rdev: 0,
        flags: 0,
        blksize: 512,
    },
    FileAttr {
        ino: 2,
        size: 65535,
        blocks: 1,
        atime: UNIX_EPOCH, // 1970-01-01 00:00:00
        mtime: UNIX_EPOCH,
        ctime: UNIX_EPOCH,
        crtime: UNIX_EPOCH,
        kind: FileType::RegularFile,
        perm: 0o644,
        nlink: 1,
        uid: 501,
        gid: 20,
        rdev: 0,
        flags: 0,
        blksize: 512,
    },
];

struct Zero {
    file: File,
    attrs: Vec<FileAttr>,
    buffer: Mmap,
}

unsafe fn zero(name: String) -> Result<Zero> {
    let mut attrs = Vec::from(ATTRS);
    let mut file = File::open(&name)?;
    attrs[1].size = file.metadata()?.len();
    let ans = memmap::MmapOptions::new().map(&file)?;
    println!("mmap len {}", ans.len());

    return Ok(Zero{
        file: file,
        attrs: attrs,
        buffer: ans,
    })
}

impl Filesystem for Zero {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if parent == 1 && name.to_str() == Some("hello.txt") {
            reply.entry(&TTL, &self.attrs[1], 0);
        } else {
            reply.error(ENOENT);
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        match ino {
            1 | 2 => reply.attr(&TTL, &self.attrs[(ino - 1) as usize]),
            _ => reply.error(ENOENT),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        match ino {
            1 => {
                vec![
                    (1, FileType::Directory, "."),
                    (1, FileType::Directory, ".."),
                    (2, FileType::RegularFile, "hello.txt"),
                ]
                .iter()
                .enumerate()
                .all(|(index, entry)| reply.add(entry.0, (index + 1) as i64, entry.1, entry.2));
                reply.ok();
            }
            _ => reply.error(ENOENT),
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        _size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        match ino {
            2 => {
                let end = min(offset as usize + _size as usize, self.buffer.len() as usize);
                let vec = self.buffer[offset as usize..end].to_owned();
                reply.data(&vec);
            }
            _ => reply.error(ENOENT),
        }
    }
}

fn main() {
    let matches = App::new("hello")
        .version(crate_version!())
        .author("Christopher Berner")
        .arg(
            Arg::with_name("MOUNT_POINT")
                .required(true)
                .index(1)
                .help("Act as a client, and mount FUSE at given path"),
        )
        .arg(
            Arg::with_name("auto_unmount")
                .long("auto_unmount")
                .help("Automatically unmount on process exit"),
        )
        .arg(
            Arg::with_name("allow-root")
                .long("allow-root")
                .help("Allow root user to access filesystem"),
        ).arg(
        Arg::with_name("datafile")
            .long("data-file").required(true).takes_value(true)
            .help("data-file for fuse server"),
        ).get_matches();
    env_logger::init();
    let mountpoint = matches.value_of("MOUNT_POINT").unwrap();
    let file = matches.value_of("datafile").unwrap();
    let mut options = vec![MountOption::RO, MountOption::FSName("hello".to_string())];
    if matches.is_present("auto_unmount") {
        options.push(MountOption::AutoUnmount);
    }
    if matches.is_present("allow-root") {
        options.push(MountOption::AllowRoot);
    }
    unsafe {
        fuser::mount2(zero(file.to_string()).unwrap(), mountpoint, &options).unwrap();
    }
}
