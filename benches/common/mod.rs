//! Core pinning, shared by all three benchmark binaries.
//!
//! `DEPTHBOOK_PIN=2` confines the run to logical CPU 2; unset or empty means unpinned,
//! which is the default and stays the default.
//!
//! Read this before trusting it to make a run reproducible. `sched_setaffinity` confines
//! *this* thread to a core; it does not reserve that core, and anything else on the
//! machine may still be scheduled there. Measured on this repository's benchmarks,
//! pinning changed nothing on an idle machine, and made results markedly *less* stable on
//! a loaded one, because the run then depends on whether another process happens to land
//! on the chosen core. Left alone, the scheduler finds an idle core more reliably than a
//! fixed choice does. Pinning earns its place only when the core is genuinely reserved,
//! with `isolcpus` or a cpuset; see CONTRIBUTING.md.
//!
//! The value is a comma-separated list, so it is also a usable mask for a benchmark that
//! spawns threads: children inherit the parent's affinity, so name at least as many cores
//! as the benchmark has threads or they will contend for one core.

#![allow(dead_code)]

/// Cores named by `DEPTHBOOK_PIN`.
///
/// Unset, empty, or naming nothing parseable is `None`, which means "do not pin" rather
/// than "pin to no cores" — the latter would be an empty mask and a failed call.
pub fn pin_list() -> Option<Vec<usize>> {
    std::env::var("DEPTHBOOK_PIN")
        .ok()
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .filter(|cores: &Vec<usize>| !cores.is_empty())
}

/// Pin the calling thread to one core, for a future benchmark that pins per thread.
pub fn pin_to(core: usize) {
    pin_mask(&[core]);
}

/// Confine the calling thread, and any thread it later spawns, to `cores`.
///
/// Linux only; a no-op elsewhere so the benchmarks still build and run, just unpinned.
pub fn pin_mask(cores: &[usize]) {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: cpu_set_t is plain data, zero-initialised as libc documents; the macros
        // and the call only read and write that local.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_ZERO(&mut set);
            for &core in cores {
                // CPU_SET indexes a fixed-size bitmap and panics out of range; check here
                // so a fat-fingered DEPTHBOOK_PIN names itself in the message. A core
                // that is in range but absent on this machine fails the call below.
                assert!(
                    core < libc::CPU_SETSIZE as usize,
                    "DEPTHBOOK_PIN names core {core}, above the {} the kernel allows",
                    libc::CPU_SETSIZE
                );
                libc::CPU_SET(core, &mut set);
            }
            let rc = libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
            // A pin that silently fails produces numbers that look fine and are not.
            assert_eq!(
                rc,
                0,
                "sched_setaffinity to {cores:?} failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = cores;
}

/// Pin from `DEPTHBOOK_PIN` and say so on stdout, so a pasted result always records how
/// it was measured. Call once, before criterion starts.
pub fn pin_from_env() {
    match pin_list() {
        Some(cores) => {
            pin_mask(&cores);
            println!("pinned to cores {cores:?}");
        }
        None => println!("unpinned (set DEPTHBOOK_PIN=2 to pin)"),
    }
}
