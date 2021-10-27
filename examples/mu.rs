use std::str::FromStr;
use structopt::StructOpt;
use std::io;
use std::env;
use std::process::Command;
use log::*;
use std::thread::sleep;
use std::time::Duration;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use flexi_logger::{colored_opt_format, Logger};
use std::os::unix::io::{RawFd, FromRawFd, AsRawFd};

use fuser::MountOption;
use fuser::{channel::Channel, mnt::Mount, Session};

fn main() {
    let opt: Options = Options::from_args();
    Logger::try_with_env_or_str("trace")
        .unwrap()
        .format(colored_opt_format)
        .start().unwrap();
    log::set_max_level(LevelFilter::Trace);
    debug!("{:?}", opt);

    match opt.role {
        Role::Master => master(opt),
        Role::Worker => worker(opt),
    }.unwrap()
}

fn master(mut opt: Options) -> io::Result<()> {
    let options = vec![
        MountOption::RO,
        MountOption::FSName("rofs".to_string()),
        MountOption::Subtype("FUSE".to_string()),
        MountOption::Async,
        MountOption::DirSync,
        MountOption::AutoUnmount,
    ];
    let (file, mount): (Arc<File>, Mount) = Mount::new((&opt.mountpoint).as_ref(), &options)?;
    let fd = file.as_ref().as_raw_fd() as i32;

    let mut child_opt = opt.clone();
    child_opt.role = Role::Worker;
    child_opt.session = fd;

    let current_dir = env::current_dir().unwrap().to_str().unwrap().to_string();
    let current_cmd = env::args().nth(0).unwrap();
    info!("{}/{}", current_dir, current_cmd);
    loop {
        let mut cmd = Command::new(format!("{}/{}", current_dir, current_cmd));
        cmd.args(child_opt.to_args());
        let mut res = cmd.spawn().expect("worker failed");
        match res.wait() {
            Ok(s) => println!("{}", s),
            Err(e) => println!("{}", e),
        }
    }
}

fn worker(opt: Options) -> io::Result<()> {
    let zerofs = unsafe{mufs::zero("file".to_string())?};
    let file = unsafe {File::from_raw_fd(opt.session as RawFd)};
    let ch = Channel::new(Arc::new(file));
    Session::restore(zerofs, opt.mountpoint.parse().unwrap(), ch).run();
    return Ok(())
}

#[derive(StructOpt, Debug, Clone)]
#[structopt(
    name = format!("test"),
)]
pub struct Options {
    #[structopt(
        short = "r",
        long = "role",
        required = true,
        help = "role of master/worker",
        default_value = "single"
    )]
    pub role: Role,
    #[structopt(
        short = "s",
        long = "session-fd",
        required = false,
        help = "fd of fuse session",
        default_value = "-1"
    )]
    pub session: i32,
    #[structopt(
        short = "p",
        long = "mountpoint",
        required = true,
        help = "mount point",
    )]
    pub mountpoint: String,
}

impl Options {
    fn to_args(&self) -> Vec<String> {
        let mut args: Vec<String> = vec![];
        args.push("--role".to_string());
        args.push(self.role.to_string());
        args.push("--session-fd".to_string());
        args.push(format!("{}", self.session));
        args.push("--mountpoint".to_string());
        args.push( self.mountpoint.to_string());
        args
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Role {
    Master,
    Worker,
}

impl FromStr for Role {
    type Err = String;
    fn from_str(role: &str) -> Result<Role, Self::Err> {
        match role {
            "master" => Ok(Role::Master),
            "worker" => Ok(Role::Worker),
            _ => Err(format!("bad role {}", role))
        }
    }
}

impl ToString for Role {
    fn to_string(&self) -> String {
        match self {
            Role::Master => "master",
            Role::Worker => "worker",
        }.parse().unwrap()
    }
}

pub mod mufs {
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
    const HELLO_TXT_CONTENT: &str = "Hello World!\n";
    const FAST_CONTENT: &str = "fast\n";
    const SLOW_CONTENT: &str = "slow\n";

    static ATTRS: [FileAttr; 4] = [
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
        }, FileAttr {
            ino: 2,
            size: 13,
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
        }, FileAttr {
            ino: 3,
            size: 5,
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
        }, FileAttr {
            ino: 4,
            size: 5,
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
        }
    ];

    pub struct Zero {
        attrs: Vec<FileAttr>,
    }

    pub unsafe fn zero(name: String) -> Result<Zero> {
        let mut attrs = Vec::from(ATTRS);

        return Ok(Zero{
            attrs: attrs,
        })
    }

    impl Filesystem for Zero {
        fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
            if parent == 1 {
                match name.to_str().unwrap() {
                    "hello.txt" => reply.entry(&TTL, &self.attrs[1], 0),
                    "fast.txt" => reply.entry(&TTL, &self.attrs[2], 0),
                    "slow.txt" => reply.entry(&TTL, &self.attrs[3], 0),
                    _ => {reply.error(ENOENT)}
                }
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
                        (3, FileType::RegularFile, "fast.txt"),
                        (4, FileType::RegularFile, "slow.txt"),
                    ]
                        .iter()
                        .enumerate()
                        .skip(offset as usize)
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
                2 => reply.data(&HELLO_TXT_CONTENT.as_bytes()[offset as usize..]),
                3 => {std::thread::sleep(Duration::from_secs(1));  reply.data(&FAST_CONTENT.as_bytes()[offset as usize..])},
                4 => {std::thread::sleep(Duration::from_secs(10)); reply.data(&SLOW_CONTENT.as_bytes()[offset as usize..])},
                _ => reply.error(ENOENT),
            }
        }
    }
}
