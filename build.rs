fn main() {
    use std::env;
    use std::path::PathBuf;

    use std::process::Command;

    println!("cargo:include=vendor/liburing/src/include/");
    println!("cargo:include=vendor/rdma-core/build/include");
    println!("cargo:include=vendor/rdma-core/kernel-headers");
    println!("cargo:rustc-link-search=native=vendor/liburing/src");
    println!("cargo:rustc-link-search=native=vendor/rdma-core/build/lib");
    println!("cargo:rustc-link-lib=uring");
    println!("cargo:rustc-link-lib=ibverbs");
    println!("cargo:rerun-if-env-changed=vendor/liburing/src/include/liburing.h");
    println!("cargo:rerun-if-env-changed=vendor/rdma-core/libibverbs/verbs.h");

    Command::new("make")
        .current_dir("vendor/liburing/")
        .args(&["CFLAGS=-O2"])
        .status()
        .expect("Failed to build liburing");

    Command::new("bash")
        .current_dir("vendor/rdma-core/")
        .args(&["build.sh"])
        .status()
        .expect("Failed to build vendor/rdma-core using build.sh");

    const INCLUDE: &str = r#"
#include <liburing.h>
#include <libibverbs/verbs.h>
#include <kernel-headers/rdma/ib_user_verbs.h>
    "#;

    #[cfg(not(feature = "overwrite"))]
    let outdir = PathBuf::from(env::var("OUT_DIR").unwrap());

    #[cfg(feature = "overwrite")]
    let outdir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/sys");

    bindgen::Builder::default()
        .header_contents("include-file.h", INCLUDE)
        .clang_arg("-Ivendor/liburing/src/include/")
        .clang_arg("-Lvendor/liburing/src")
        .clang_arg("-Ivendor/rdma-core/")
        // .ctypes_prefix("libc")
        .generate_comments(true)
        .use_core()
        .whitelist_type("io_uring_.*")
        .whitelist_function("io_uring_.*")
        .whitelist_var("IORING_OP_.*")
        .whitelist_function("__io_uring_.*")
        .whitelist_function("ibv_.*")
        .whitelist_type("ibv_.*")
        .whitelist_type("ib_uverbs_comp_event_desc")
        .whitelist_var("IBV.*")
        .constified_enum_module("ibv_qp_type")
        .constified_enum_module("ibv_qp_state")
        .constified_enum_module("ibv_qp_attr_mask")
        .constified_enum_module("ibv_access_flags")
        .constified_enum_module("ibv_wr_opcode")
        .constified_enum_module("ibv_send_flags")
        .constified_enum_module("ibv_wc_opcode")
        .generate()
        .unwrap()
        .write_to_file(outdir.join("sys.rs"))
        .unwrap();
}
