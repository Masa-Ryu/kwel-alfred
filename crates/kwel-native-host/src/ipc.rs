use kwel_protocol::{Code, Error, MAX_MESSAGE_BYTES, Result, encode_frame, frame_length};
use serde::Serialize;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
pub type Connection = Box<dyn Stream>;
pub async fn read(
    stream: &mut (impl AsyncRead + Unpin + ?Sized),
    maximum: usize,
) -> Result<Vec<u8>> {
    let mut header = [0; 4];
    stream.read_exact(&mut header).await?;
    let mut payload = vec![0; frame_length(header, maximum)?];
    stream.read_exact(&mut payload).await?;
    Ok(payload)
}
pub async fn write(
    stream: &mut (impl AsyncWrite + Unpin + ?Sized),
    value: &impl Serialize,
) -> Result<()> {
    stream
        .write_all(&encode_frame(value, MAX_MESSAGE_BYTES)?)
        .await?;
    stream.flush().await?;
    Ok(())
}
#[cfg(unix)]
pub struct Listener {
    listener: tokio::net::UnixListener,
    path: PathBuf,
    identity: (u64, u64),
}
#[cfg(unix)]
impl Listener {
    pub fn bind(path: &Path) -> Result<Self> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
        crate::secure::verify_dir(path.parent().ok_or(Error(Code::UnsafePath))?)?;
        if let Ok(before) = std::fs::symlink_metadata(path) {
            if !before.file_type().is_socket()
                || before.uid() != crate::secure::uid()
                || before.mode() & 0o777 != 0o600
            {
                return Err(Error(Code::UnsafePath));
            }
            match std::os::unix::net::UnixStream::connect(path) {
                Ok(_) => return Err(Error(Code::Busy)),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    let after = std::fs::symlink_metadata(path)?;
                    if (before.dev(), before.ino()) != (after.dev(), after.ino()) {
                        return Err(Error(Code::UnsafePath));
                    }
                    std::fs::remove_file(path)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let listener = tokio::net::UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        let metadata = std::fs::symlink_metadata(path)?;
        Ok(Self {
            listener,
            path: path.to_owned(),
            identity: (metadata.dev(), metadata.ino()),
        })
    }
    pub async fn accept(&mut self) -> Result<Connection> {
        let (stream, _) = self.listener.accept().await?;
        if stream.peer_cred()?.uid() != crate::secure::uid() {
            return Err(Error(Code::PermissionRequired));
        }
        Ok(Box::new(stream))
    }
}
#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        if let Ok(now) = std::fs::symlink_metadata(&self.path)
            && (now.dev(), now.ino()) == self.identity
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
#[cfg(unix)]
pub async fn connect(path: &Path) -> Result<Connection> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    crate::secure::verify_dir(path.parent().ok_or(Error(Code::UnsafePath))?)?;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != crate::secure::uid()
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(Error(Code::UnsafePath));
    }
    let stream = tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::UnixStream::connect(path),
    )
    .await
    .map_err(|_| Error(Code::Timeout))??;
    if stream.peer_cred()?.uid() != crate::secure::uid() {
        return Err(Error(Code::PermissionRequired));
    }
    Ok(Box::new(stream))
}
#[cfg(windows)]
pub struct Listener {
    server: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
    path: PathBuf,
}
#[cfg(windows)]
impl Listener {
    pub fn bind(path: &Path) -> Result<Self> {
        Ok(Self {
            server: Some(crate::windows::pipe_server(path, true)?),
            path: path.to_owned(),
        })
    }
    pub async fn accept(&mut self) -> Result<Connection> {
        use std::os::windows::io::AsRawHandle;
        let server = self.server.take().ok_or(Error(Code::Disconnected))?;
        server.connect().await?;
        self.server = Some(crate::windows::pipe_server(&self.path, false)?);
        crate::windows::verify_pipe_client(server.as_raw_handle())?;
        Ok(Box::new(server))
    }
}
#[cfg(windows)]
pub async fn connect(path: &Path) -> Result<Connection> {
    use std::os::windows::io::AsRawHandle;
    let name = path.as_os_str().to_string_lossy();
    if !name.starts_with(r"\\.\pipe\kwel-") {
        return Err(Error(Code::UnsafePath));
    }
    let started = std::time::Instant::now();
    let stream = loop {
        match tokio::net::windows::named_pipe::ClientOptions::new()
            .security_qos_flags(windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION)
            .open(path)
        {
            Ok(stream) => break stream,
            Err(error)
                if error.raw_os_error()
                    == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32) =>
            {
                let remaining = 500u128.saturating_sub(started.elapsed().as_millis());
                if remaining == 0 {
                    return Err(Error(Code::Timeout));
                }
                let path = path.to_owned();
                tokio::task::spawn_blocking(move || {
                    crate::windows::wait_pipe(&path, remaining as u32)
                })
                .await
                .map_err(|_| Error(Code::Disconnected))??;
            }
            Err(error) => return Err(error.into()),
        }
    };
    crate::windows::verify_pipe_server(stream.as_raw_handle())?;
    Ok(Box::new(stream))
}
