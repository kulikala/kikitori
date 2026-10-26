use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Mutex,
    thread,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn request(stream: &mut std::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut b = [0; 1024];
    loop {
        let n = stream.read(&mut b).unwrap();
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&b[..n]);
        if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
fn reply(stream: &mut std::net::TcpStream, headers: &str, body: &[u8]) {
    write!(stream, "HTTP/1.1 {headers}\r\nConnection: close\r\n\r\n").unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
}

#[test]
fn resumes_after_cut_and_replays_range_after_redirect() {
    let dest = TcpListener::bind("127.0.0.1:0").unwrap();
    let dest_addr = dest.local_addr().unwrap();
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_addr = origin.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = origin.accept().unwrap();
        let _ = request(&mut s);
        reply(&mut s, "200 OK\r\nContent-Length: 6", b"abc");
        drop(s);
        let (mut s, _) = origin.accept().unwrap();
        let req = request(&mut s);
        assert!(req.to_ascii_lowercase().contains("range: bytes=3-"));
        reply(
            &mut s,
            &format!("302 Found\r\nLocation: http://{dest_addr}/file\r\nContent-Length: 0"),
            b"",
        );
        drop(s);
        let (mut s, _) = dest.accept().unwrap();
        let req = request(&mut s);
        assert!(req.to_ascii_lowercase().contains("range: bytes=3-"));
        reply(
            &mut s,
            "206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6",
            b"def",
        );
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("download");
    let hash = hex::encode(Sha256::digest(b"abcdef"));
    download(
        &format!("http://{origin_addr}/file"),
        &path,
        Some(6),
        Some(&hash),
        false,
        |_, _| {},
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(fs::read(path).unwrap(), b"abcdef");
}

#[test]
fn checksum_mismatch_never_places_destination() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for _ in 0..4 {
            let (mut s, _) = listener.accept().unwrap();
            let _ = request(&mut s);
            reply(&mut s, "200 OK\r\nContent-Length: 3", b"bad");
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("file");
    assert!(download(
        &format!("http://{addr}/"),
        &dest,
        Some(3),
        Some(&hex::encode(Sha256::digest(b"good"))),
        false,
        |_, _| {}
    )
    .is_err());
    server.join().unwrap();
    assert!(!dest.exists());
}

#[test]
fn reuses_hf_blobs_by_git_oid_and_snapshot_by_sha256() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::env::set_var("HF_HUB_CACHE", root.path());
    let repo = "example/model";
    let dir = root.path().join("models--example--model");
    fs::create_dir_all(dir.join("blobs")).unwrap();
    let data = b"blob content";
    let sha = hex::encode(Sha256::digest(data));
    let mut gh = sha1::Sha1::new();
    gh.update(format!("blob {}\0", data.len()).as_bytes());
    gh.update(data);
    let git = hex::encode(gh.finalize());
    fs::write(dir.join("blobs").join(&git), data).unwrap();
    let snap = fetch_hf(
        repo,
        "pin1",
        "weights.bin",
        Some(&sha),
        Some(&git),
        Some(data.len() as u64),
        true,
        |_, _| {},
    )
    .unwrap();
    assert_eq!(fs::read(&snap).unwrap(), data);
    fs::create_dir_all(dir.join("snapshots/old")).unwrap();
    fs::write(dir.join("snapshots/old/config.json"), b"same config").unwrap();
    let cfg_sha = hex::encode(Sha256::digest(b"same config"));
    let cfg = fetch_hf(
        repo,
        "pin2",
        "config.json",
        Some(&cfg_sha),
        None,
        Some(11),
        true,
        |_, _| {},
    )
    .unwrap();
    assert_eq!(fs::read(cfg).unwrap(), b"same config");
    std::env::remove_var("HF_HUB_CACHE");
}

#[test]
fn parallel_receipt_appends_keep_complete_lines() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_CONFIG_HOME", root.path());
    let threads: Vec<_> = (0..16)
        .map(|i| {
            thread::spawn(move || record_receipt("models-dir", &format!("/tmp/model-{i}")).unwrap())
        })
        .collect();
    for t in threads {
        t.join().unwrap()
    }
    let lines = fs::read_to_string(receipt_path()).unwrap();
    assert_eq!(lines.lines().count(), 16);
    assert!(lines.lines().all(|l| l
        .split_once('\t')
        .is_some_and(|(k, v)| k == "models-dir" && v.starts_with("/tmp/model-"))));
    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn dictionary_is_taken_from_the_sdist_layout_and_staging_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("u.tar.gz");
    {
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&archive).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        for name in ["sys.dic", "matrix.bin"] {
            let body = name.as_bytes();
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(
                &mut h,
                format!("unidic-lite-1.0.8/unidic_lite/dicdir/{name}"),
                body,
            )
            .unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }
    let target = dir.path().join("dict/unidic-lite-1.0.8");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    crate::extract_dictionary(&archive, &target).unwrap();
    assert_eq!(std::fs::read(target.join("sys.dic")).unwrap(), b"sys.dic");
    let left: Vec<_> = std::fs::read_dir(target.parent().unwrap())
        .unwrap()
        .collect();
    assert_eq!(left.len(), 1, "only the dictionary itself stays");
}
