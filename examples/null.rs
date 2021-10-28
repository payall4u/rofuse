use rofuse::{Filesystem, MountOption};
use std::env;

struct NullFS;

impl Filesystem for NullFS {}

fn main() {
    env_logger::init();
    let mountpoint = env::args_os().nth(1).unwrap();
    rofuse::mount2(NullFS, mountpoint, &[MountOption::AutoUnmount]).unwrap();
}
