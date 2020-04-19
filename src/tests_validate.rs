use super::*;
use crate::crypto::KeyLoader;
use std::io::prelude::*;

fn test_rpm_file_path() -> std::path::PathBuf {
    let mut rpm_path = cargo_manifest_dir();
    rpm_path.push("test_assets/389-ds-base-devel-1.3.8.4-15.el7.x86_64.rpm");
    rpm_path
}

fn cargo_manifest_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cargo_out_dir() -> std::path::PathBuf {
    cargo_manifest_dir().join("target")
}

#[cfg(feature = "signing-meta")]
use crypto::{self, algorithm::RSA, Signing, Verifying};

#[cfg(feature = "signing-pgp")]
mod pgp {
    use super::*;
    use crypto::pgp::{Signer, Verifier};

    #[test]
    fn create_full_rpm_with_signature_and_verify_externally() {
        let _ = env_logger::try_init();
        let (signing_key, _) = crate::crypto::test::load_asc_keys();
        super::create_full_rpm::<Signer>(&signing_key)
            .expect("create_full_rpm_with_signature_and_verify_externally> failed")
    }

    #[test]
    fn parse_externally_signed_rpm_and_verify() {
        let _ = env_logger::try_init();
        let (_, verification_key) = crate::crypto::test::load_asc_keys();
        super::verify_signed_rpm::<Verifier>(&verification_key)
            .expect("parse_externally_signed_rpm_and_verify> failed")
    }

    #[test]
    fn create_signed_rpm_and_verify() {
        let _ = env_logger::try_init();
        let (signing_key, verification_key) = crate::crypto::test::load_asc_keys();
        super::roundtrip::<Signer, Verifier>(signing_key.as_slice(), verification_key.as_slice())
            .expect("create_signed_rpm_and_verify> failed")
    }

    #[test]
    fn create_signature_with_gpg_and_verify() {
        let _ = env_logger::try_init();
        let (signing_key, verification_key) = crate::crypto::test::load_asc_keys();

        let test_file = cargo_out_dir().join("test.file");
        let test_file_sig = cargo_out_dir().join("test.file.sig");

        std::fs::write(&test_file, "test").expect("Must be able to write");
        let _ = std::fs::remove_file(&test_file_sig);

        let cmd=
            r#"
    ls -al /out/test.file*
    chmod +rwx /out/test.file*

    echo "test" > /out/test.file

    echo ">>> sign like rpm"
    echo "cmd: $(rpm --define "__signature_filename /out/test.file.sig" \
         --define "__plaintext_filename /out/test.file" \
         --define "_gpg_name \"Package Manager\"" \
         --eval "%{__gpg_sign_cmd}" | sd '\n' ' ')"

    gpg --verbose --no-armor --batch --pinentry-mode error --no-secmem-warning -u "Package Manager" -sbo /out/test.file.sig /out/test.file

    echo ">>> inspect signature"
    gpg -d --batch --pinentry-mode error --no-secmem-warning  < /out/test.file.sig

    echo ">>> verify external gpg signature"
    gpg --verify /out/test.file.sig /out/test.file

    "#.to_owned();

        podman_container_launcher(cmd.as_str(), "fedora:31", vec![]).expect("Container execution must be flawless");

        let v = Verifier::load_from(verification_key.as_slice()).expect("Must load");

        let raw_sig = std::fs::read(&test_file_sig).expect("must laod signature");
        let data = std::fs::read(&test_file).expect("must laod file");
        v.verify(data .as_slice(),raw_sig.as_slice()).expect("Must verify");
    }
}

use std::io::BufReader;
use std::process::Stdio;

fn roundtrip<S, V>(
    signing_key: &[u8],
    verififcation_key: &[u8],
) -> Result<(), Box<dyn std::error::Error>>
where
    S: Signing<RSA, Signature = Vec<u8>> + KeyLoader<crypto::key::Secret>,
    V: Verifying<RSA, Signature = Vec<u8>> + KeyLoader<crypto::key::Public>,
{
    let cargo_file = cargo_manifest_dir().join("Cargo.toml");
    let out_file = cargo_out_dir().join("roundtrip.rpm");

    {
        let mut f = std::fs::File::create(&out_file)?;
        let pkg = RPMBuilder::new(
            "roundtrip",
            "1.0.0",
            "MIT",
            "x86_64",
            "spins round and round",
        )
        .compression(Compressor::from_str("gzip")?)
        .with_file(
            cargo_file.to_str().unwrap(),
            RPMFileOptions::new("/etc/foobar/hugo/bazz.toml")
                .mode(0o100777)
                .is_config(),
        )?
        .with_file(
            cargo_file.to_str().unwrap(),
            RPMFileOptions::new("/etc/Cargo.toml"),
        )?
        .epoch(3)
        .pre_install_script("echo preinst")
        .add_changelog_entry("you", "yada yada", 12317712)
        .requires(Dependency::any("rpm-sign".to_string()))
        .build_and_sign::<S>(signing_key)?;

        pkg.write(&mut f)?;
        let epoch = pkg.metadata.header.get_epoch()?;
        assert_eq!(3, epoch);
    }

    // verify
    {
        let out_file = std::fs::File::open(&out_file).expect("should be able to open rpm file");
        let mut buf_reader = std::io::BufReader::new(out_file);
        let package = RPMPackage::parse(&mut buf_reader)?;
        package.verify_signature::<V>(verififcation_key)?;
    }
    Ok(())
}

fn create_full_rpm<S>(gpg_signing_key: &[u8]) -> Result<(), Box<dyn std::error::Error>>
where
    S: Signing<RSA, Signature = Vec<u8>> + KeyLoader<crypto::key::Secret>,
{
    let cargo_file = cargo_manifest_dir().join("Cargo.toml");
    let out_file = cargo_out_dir().join("test.rpm");

    let mut f = std::fs::File::create(out_file)?;
    let pkg = RPMBuilder::new("test", "1.0.0", "MIT", "x86_64", "some package")
        .compression(Compressor::from_str("gzip")?)
        .with_file(
            cargo_file.to_str().unwrap(),
            RPMFileOptions::new("/etc/foobar/foo.toml"),
        )?
        .with_file(
            cargo_file.to_str().unwrap(),
            RPMFileOptions::new("/etc/foobar/zazz.toml"),
        )?
        .with_file(
            cargo_file.to_str().unwrap(),
            RPMFileOptions::new("/etc/foobar/hugo/bazz.toml")
                .mode(0o100777)
                .is_config(),
        )?
        .with_file(
            cargo_file.to_str().unwrap(),
            RPMFileOptions::new("/etc/foobar/bazz.toml"),
        )?
        .with_file(
            cargo_file.to_str().unwrap(),
            RPMFileOptions::new("/etc/foobar/hugo/aa.toml"),
        )?
        .with_file(
            cargo_file.to_str().unwrap(),
            RPMFileOptions::new("/var/honollulu/bazz.toml"),
        )?
        .with_file(
            cargo_file.to_str().unwrap(),
            RPMFileOptions::new("/etc/Cargo.toml"),
        )?
        .epoch(1)
        .pre_install_script("echo preinst")
        .add_changelog_entry("me", "was awesome, eh?", 123123123)
        .add_changelog_entry("you", "yeah, it was", 12312312)
        .requires(Dependency::any("rpm-sign".to_string()))
        .build_and_sign::<S>(gpg_signing_key)?;

    pkg.write(&mut f)?;
    let epoch = pkg.metadata.header.get_epoch()?;
    assert_eq!(1, epoch);

    let yum_cmd = "yum --disablerepo=updates,updates-testing,updates-modular,fedora-modular install -y /out/test.rpm;";
    let dnf_cmd = "dnf --disablerepo=updates,updates-testing,updates-modular,fedora-modular install -y /out/test.rpm;";
    let rpm_sig_check = format!("rpm --verbose --checksig /out/test.rpm 2>&1;");

    [
        ("fedora:31", rpm_sig_check.as_str()),
        ("fedora:31", dnf_cmd),
        ("centos:8", yum_cmd),
        ("centos:7", yum_cmd),
    ]
    .iter()
    .try_for_each(|(image, cmd)| {
        podman_container_launcher(cmd, image, vec![])?;
        Ok(())
    })
}

fn verify_signed_rpm<V>(verification_key: &[u8]) -> Result<(), Box<dyn std::error::Error>>
where
    V: Verifying<RSA, Signature = Vec<u8>> + KeyLoader<crypto::key::Public>,
{
    let rpm_file_path = test_rpm_file_path();
    let out_file = cargo_out_dir().join(rpm_file_path.file_name().unwrap().to_str().unwrap());

    println!("cpy {} -> {}", rpm_file_path.display(), out_file.display());
    std::fs::copy(rpm_file_path.as_path(), out_file.as_path()).expect("Must be able to copy");

    let cmd = format!(
        r#"
echo ">>> sign"
rpm --verbose --addsign /out/{rpm_file} 2>&1

echo ">>> verify"
rpm --verbose --checksig /out/{rpm_file} 2>&1
"#,
        rpm_file = rpm_file_path.file_name().unwrap().to_str().unwrap()
    );

    podman_container_launcher(cmd.as_str(), "fedora:31", vec![])?;

    let out_file = std::fs::File::open(&rpm_file_path).expect("should be able to open rpm file");
    let mut buf_reader = std::io::BufReader::new(out_file);
    let package = RPMPackage::parse(&mut buf_reader)?;
    package
        .verify_signature::<V>(verification_key.as_ref())
        .expect("Key should verify rpm");

    Ok(())
}



fn wait_and_print_helper(mut child: std::process::Child, stdin_cmd: &str) -> std::io::Result<()> {
    if let Some(ref mut stdin) = child.stdin {
        write!(stdin, "{}", stdin_cmd).unwrap();
    } else {
        unreachable!("Must have stdin");
    }
    // not perfect, but gets it done
    if let Some(ref mut stdout) = child.stdout {
        if let Some(ref mut stderr) = child.stderr {
            let stdout_rdr = BufReader::new(stdout);
            let mut stdout_line = stdout_rdr.lines();

            let stderr_rdr = BufReader::new(stderr);
            let mut stderr_line = stderr_rdr.lines();

            let mut done: bool = false;
            while !done {
                done = true;
                while let Some(line) = stdout_line.next() {
                    done = false;
                    println!("[stdout] {}", line.unwrap().as_str());
                }
                while let Some(line) = stderr_line.next() {
                    done = false;
                    println!("[stderr] {}", line.unwrap().as_str());
                }
            }
        } else {
            unreachable!("Must have stderr");
        }
    } else {
        unreachable!("Must have stdout");
    }

    let status = child.wait()?;
    assert!(status.success());
    Ok(())
}

fn podman_container_launcher(
    cmd: &str,
    image: &str,
    mut mappings: Vec<String>,
) -> std::io::Result<()> {
    // always mount assets and out directory into container
    let var_cache = cargo_manifest_dir().join("dnf-cache");
    let _ = std::fs::create_dir(var_cache.as_path());
    let var_cache = format!("{}:/var/cache/dnf:z", var_cache.display());
    let out = format!("{}:/out:z", cargo_out_dir().display());
    let assets = format!("{}/test_assets:/assets:z", cargo_manifest_dir().display());
    mappings.extend(vec![out, assets, var_cache]);
    let mut args = mappings
        .iter()
        .fold(vec!["run", "-i", "--rm"], |mut acc, mapping| {
            acc.extend(vec!["-v", mapping]);
            acc
        });
    args.extend(vec![image, "sh"]);

    let mut podman_cmd = std::process::Command::new("podman");

    podman_cmd.args(dbg!(args));
    podman_cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    podman_cmd.stdin(Stdio::piped());

    podman_cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    podman_cmd.stdin(Stdio::piped());

    // partially following:
    //
    //  https://access.redhat.com/articles/3359321
    let cmd = vec![
        r#"
set -e

# prepare rpm macros

cat > ~/.rpmmacros << EOF_RPMMACROS
%_signature gpg
%_gpg_path /root/.gnupg
%_gpg_name Package Manager
%_gpgbin /usr/bin/gpg2
%__gpg_sign_cmd %{__gpg} \
    gpg \
    --verbose \
    --no-armor \
    --batch \
    --pinentry-mode error \
    %{?_gpg_digest_algo:--digest-algo %{_gpg_digest_algo}} \
    --no-secmem-warning \
    -u "%{_gpg_name}" \
    -sbo %{__signature_filename} \
    %{__plaintext_filename}
EOF_RPMMACROS

cat ~/.rpmmacros

### either

#cat > gpgkeyspec <<EOF
#     %echo Generating a basic OpenPGP key
#     Key-Type: RSA
#     Key-Length: 2048
#     Subkey-Type: RSA
#     Subkey-Length: 2048
#     Name-Real: Package Manager
#     Name-Comment: unprotected
#     Name-Email: pmanager@example.com
#     Expire-Date: 0
#     %no-ask-passphrase
#     %no-protection
#     %commit
#     %echo done
#EOF
#gpg --batch --generate-key gpgkeyspec  2>&1

### or (which has a couple of advantages regarding reproducability)

export PK=/assets/id_rsa.pub.asc
export SK=/assets/id_rsa.asc

gpg --import "${SK}" 2>&1

###

gpg --with-keygrip --list-keys  2>&1

echo "\### create a test signature with this particular key id"

echo "test" | gpg -s --local-user CFD331925AB27F39 > /tmp/test.signature 2>&1
gpg -d < /tmp/test.signature   2>&1

echo "\### export PK"

gpg --export -a "Package Manager" > /assets/RPM-GPG-KEY-pmanager

dig1=$(sha256sum "/assets/RPM-GPG-KEY-pmanager")
dig2=$(sha256sum "${PK}")

if [ "$dig1" = "$dig2" ]; then
    echo "\### expected pub key and exported pubkey differ"
    exit 77
fi

echo "\### install tooling for signing"

dnf install --disablerepo=updates,updates-testing,updates-modular -y rpm-sign sd || \
yum install --disablerepo=updates,updates-testing,updates-modular -y rpm-sign

echo "\### import pub key"

rpm --verbose --import "${PK}" 2>&1

set -x

"#,
cmd,
r#"

echo "\### Container should exit any second now"
exit 0
"#].join("\n");


    // this is far from perfect, but at least pumps
    // stdio and stderr out
    wait_and_print_helper(podman_cmd.spawn()?, cmd.as_str())?;
    println!("Container execution complete ;)");
    Ok(())
}