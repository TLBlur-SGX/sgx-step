use std::path::Path;

fn main() {
    #[cfg(feature = "sgx")]
    {
        let sgxsdk = std::env::var("SGX_SDK").unwrap_or_else(|_| "/opt/intel/sgxsdk".into());
        let libsgxstep =
            std::env::var("LIBSGXSTEP").unwrap_or_else(|_| "../../../libsgxstep".into());
        let enclave_dir = "../Enclave";

        assert!(Path::new(&format!("{enclave_dir}/encl_u.c")).exists());
        assert!(Path::new(&format!("{enclave_dir}/encl_u.h")).exists());

        println!("cargo:rerun-if-changed=libjpeg.c");
        println!("cargo:rerun-if-changed={enclave_dir}/encl_u.c");
        println!("cargo:rerun-if-changed={enclave_dir}/encl_u.h");

        cc::Build::new()
            .file("libjpeg.c")
            .file(&format!("{enclave_dir}/encl_u.c"))
            .flag(&format!("-I{enclave_dir}"))
            .flag(&format!("-I{sgxsdk}/include"))
            .flag(&format!("-I{libsgxstep}"))
            .cargo_metadata(true)
            .compile("attack-libjpeg");
    }
}
