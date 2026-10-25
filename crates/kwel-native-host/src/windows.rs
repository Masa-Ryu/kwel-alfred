//! The only Windows FFI boundary. Handles and LocalAlloc buffers are owned here;
//! no impersonation or raw security pointer survives an await point.
use kwel_protocol::{Code, Error, Result};
use std::{
    ffi::{OsStr, c_void},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
};
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::{
    Foundation::*, Security::Authorization::*, Security::*, Storage::FileSystem::*,
    System::Pipes::*, System::Threading::*,
};
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn failed<T>() -> Result<T> {
    Err(Error(Code::PermissionRequired))
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the valid token/process handle returned by Win32.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: Win32 allocated the descriptor with LocalAlloc.
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn sid_string(sid: PSID) -> Result<String> {
    // SAFETY: callers pass a SID borrowed from a live token or descriptor. The
    // converted null-terminated string is copied before LocalFree.
    unsafe {
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return failed();
        }
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let result = String::from_utf16(std::slice::from_raw_parts(text, length))
            .map_err(|_| Error(Code::PermissionRequired));
        LocalFree(text.cast());
        result
    }
}
fn token_sid(token: HANDLE) -> Result<String> {
    // SAFETY: token is held open by the caller. TOKEN_USER needs pointer
    // alignment; the usize buffer supplies it and its Win32-reported size.
    unsafe {
        let mut bytes = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut bytes);
        if bytes == 0 {
            return failed();
        }
        let mut buffer = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        ) == 0
        {
            return failed();
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        sid_string(user.User.Sid)
    }
}
pub fn current_sid() -> Result<String> {
    // SAFETY: GetCurrentProcess is a borrowed pseudo-handle; only the new token
    // handle is closed. TOKEN_QUERY grants no elevation or impersonation.
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return failed();
        }
        let token = Handle(token);
        token_sid(token.0)
    }
}
pub fn pipe_prefix() -> Result<String> {
    use sha2::{Digest, Sha256};
    Ok(crate::secure::hex(&Sha256::digest(current_sid()?.as_bytes()))[..24].into())
}
fn descriptor() -> Result<Descriptor> {
    let sid = current_sid()?;
    descriptor_from(&format!("O:{sid}D:P(A;OICI;FA;;;{sid})"))
}
fn descriptor_from(value: &str) -> Result<Descriptor> {
    let sddl = wide(OsStr::new(value));
    // SAFETY: sddl is NUL-terminated and valid for the call; returned memory is
    // freed by Descriptor only after pipe/file security operations finish.
    unsafe {
        let mut descriptor = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return failed();
        }
        Ok(Descriptor(descriptor))
    }
}
pub fn wait_pipe(path: &Path, milliseconds: u32) -> Result<()> {
    let path = wide(path.as_os_str());
    // SAFETY: the caller has required a local Kwel pipe name. Waiting is bounded
    // by the remaining connection deadline, and creates no inheritable handle.
    unsafe {
        if WaitNamedPipeW(path.as_ptr(), milliseconds) == 0 {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_SEM_TIMEOUT as i32) {
                Err(Error(Code::Timeout))
            } else {
                Err(error.into())
            };
        }
    }
    Ok(())
}
pub fn reject_reparse(path: &Path) -> Result<()> {
    let path = wide(path.as_os_str());
    // SAFETY: the UTF-16 path is valid and terminated; GetFileAttributesW does
    // not follow or mutate a reparse point for this attribute check.
    unsafe {
        let attributes = GetFileAttributesW(path.as_ptr());
        if attributes == INVALID_FILE_ATTRIBUTES || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Error(Code::UnsafePath));
        }
    }
    Ok(())
}
pub fn make_private(path: &Path) -> Result<()> {
    reject_reparse(path)?;
    let descriptor = descriptor()?;
    let mut path = wide(path.as_os_str());
    // SAFETY: all output pointers borrow from descriptor for the duration of
    // SetNamedSecurityInfoW; the descriptor is not freed until afterward.
    unsafe {
        let mut owner = ptr::null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        let mut dacl = ptr::null_mut();
        if GetSecurityDescriptorOwner(descriptor.0, &mut owner, &mut defaulted) == 0
            || GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) == 0
        {
            return failed();
        }
        if SetNamedSecurityInfoW(
            path.as_mut_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            owner,
            ptr::null_mut(),
            dacl,
            ptr::null_mut(),
        ) != ERROR_SUCCESS
        {
            return failed();
        }
    }
    verify_private(path_from_wide(&path).as_path())
}
fn path_from_wide(path: &[u16]) -> std::path::PathBuf {
    use std::os::windows::ffi::OsStringExt;
    std::ffi::OsString::from_wide(&path[..path.len() - 1]).into()
}
pub fn verify_private(path: &Path) -> Result<()> {
    reject_reparse(path)?;
    let expected = current_sid()?;
    let mut path = wide(path.as_os_str());
    // SAFETY: GetNamedSecurityInfoW owns its returned descriptor, which contains
    // all borrowed ACE/SID pointers. Sizes and ACE type are checked before casts.
    unsafe {
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        if GetNamedSecurityInfoW(
            path.as_mut_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        ) != ERROR_SUCCESS
        {
            return failed();
        }
        let descriptor = Descriptor(descriptor);
        let mut control = 0;
        let mut revision = 0;
        if GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) == 0
            || control & SE_DACL_PROTECTED == 0
        {
            return Err(Error(Code::UnsafePath));
        }
        if owner.is_null() || dacl.is_null() || sid_string(owner)? != expected {
            return Err(Error(Code::UnsafePath));
        }
        let mut size: ACL_SIZE_INFORMATION = std::mem::zeroed();
        if GetAclInformation(
            dacl,
            (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        ) == 0
            || size.AceCount != 1
        {
            return Err(Error(Code::UnsafePath));
        }
        let mut ace = ptr::null_mut();
        if GetAce(dacl, 0, &mut ace) == 0 {
            return failed();
        }
        let header = &*ace.cast::<ACE_HEADER>();
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8
            || (header.AceSize as usize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
        {
            return Err(Error(Code::UnsafePath));
        }
        let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
        if sid_string(ptr::addr_of!(allowed.SidStart).cast_mut().cast())? != expected {
            return Err(Error(Code::UnsafePath));
        }
        drop(descriptor);
    }
    Ok(())
}
pub fn verify_owner(path: &Path) -> Result<()> {
    reject_reparse(path)?;
    let expected = current_sid()?;
    let mut path = wide(path.as_os_str());
    // SAFETY: owner borrows from the returned LocalAlloc descriptor until its
    // SID is copied; Descriptor then releases that allocation exactly once.
    unsafe {
        let mut owner = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        if GetNamedSecurityInfoW(
            path.as_mut_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        ) != ERROR_SUCCESS
        {
            return failed();
        }
        let _descriptor = Descriptor(descriptor);
        if sid_string(owner)? != expected {
            return Err(Error(Code::UnsafePath));
        }
    }
    Ok(())
}
pub fn pipe_server(
    path: &Path,
    first: bool,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    let descriptor = descriptor()?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // SAFETY: attributes and its self-only DACL remain live until CreateNamedPipe
    // has copied them. Tokio owns the new non-inheritable overlapped handle.
    unsafe {
        Ok(tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(40)
            .create_with_security_attributes_raw(
                path,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
            )?)
    }
}
pub fn verify_pipe_server(raw: *mut c_void) -> Result<()> {
    // SAFETY: Tokio keeps this connected pipe open. The process/token wrappers
    // own only the new query handles and never the borrowed pipe handle.
    unsafe {
        let mut pid = 0;
        if GetNamedPipeServerProcessId(raw, &mut pid) == 0 {
            return failed();
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return failed();
        }
        let process = Handle(process);
        let mut token = ptr::null_mut();
        if OpenProcessToken(process.0, TOKEN_QUERY, &mut token) == 0 {
            return failed();
        }
        let token = Handle(token);
        if token_sid(token.0)? != current_sid()? {
            return failed();
        }
    }
    Ok(())
}
pub fn verify_pipe_client(raw: *mut c_void) -> Result<()> {
    let expected = current_sid()?;
    // SAFETY: impersonation is limited to this synchronous block. RevertToSelf
    // runs on every path before returning to Tokio, including a query failure.
    unsafe {
        if ImpersonateNamedPipeClient(raw) == 0 {
            return failed();
        }
        let result = (|| {
            let mut token = ptr::null_mut();
            if OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) == 0 {
                return failed();
            }
            let token = Handle(token);
            token_sid(token.0)
        })();
        if RevertToSelf() == 0 {
            std::process::abort();
        }
        if result? != expected {
            return failed();
        }
    }
    Ok(())
}
pub fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    reject_reparse(source)?;
    reject_reparse(destination)?;
    let source = wide(source.as_os_str());
    let destination = wide(destination.as_os_str());
    // SAFETY: both validated, NUL-terminated paths are held across MoveFileExW.
    unsafe {
        if MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        ) == 0
        {
            return Err(Error(Code::HostInUse));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protected_acl_rejects_everyone_and_recovers_to_current_sid_only() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap().join("private");
        crate::secure::ensure_dir(&root).unwrap();
        let path = root.join("acl-fixture");
        crate::secure::atomic_write(&path, b"fixture", false).unwrap();
        verify_private(&path).unwrap();
        let sid = current_sid().unwrap();
        let descriptor = descriptor_from(&format!("O:{sid}D:P(A;;FA;;;S-1-1-0)")).unwrap();
        let mut name = wide(path.as_os_str());
        // SAFETY: this test changes only its owned temporary file. All pointers
        // are borrowed from the live descriptor and no impersonation is used.
        unsafe {
            let mut dacl = ptr::null_mut();
            let mut present = 0;
            let mut defaulted = 0;
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted),
                0
            );
            assert_eq!(
                SetNamedSecurityInfoW(
                    name.as_mut_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    dacl,
                    ptr::null_mut()
                ),
                ERROR_SUCCESS
            );
        }
        assert!(verify_private(&path).is_err());
        make_private(&path).unwrap();
        verify_private(&path).unwrap();
    }
    #[tokio::test]
    async fn restricted_sid_cannot_open_the_private_pipe() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = crate::paths::RuntimePaths::at(
            temporary.path().canonicalize().unwrap().join("runtime"),
        )
        .unwrap();
        let name = paths
            .endpoint("00000000-0000-4000-8000-00000000000a", false)
            .unwrap();
        let _server = pipe_server(&name, true).unwrap();
        let mut original = ptr::null_mut();
        let mut restricted = ptr::null_mut();
        // SAFETY: buffers are pointer-aligned, Win32 supplies the SID length,
        // and only a restricted derivative of this process's own token is made.
        unsafe {
            assert_ne!(
                OpenProcessToken(
                    GetCurrentProcess(),
                    TOKEN_QUERY | TOKEN_DUPLICATE,
                    &mut original
                ),
                0
            );
            let original = Handle(original);
            let mut sid = [0usize; 16];
            let mut length = std::mem::size_of_val(&sid) as u32;
            assert_ne!(
                CreateWellKnownSid(
                    WinWorldSid,
                    ptr::null_mut(),
                    sid.as_mut_ptr().cast(),
                    &mut length
                ),
                0
            );
            let restriction = SID_AND_ATTRIBUTES {
                Sid: sid.as_mut_ptr().cast(),
                Attributes: 0,
            };
            assert_ne!(
                CreateRestrictedToken(
                    original.0,
                    DISABLE_MAX_PRIVILEGE,
                    0,
                    ptr::null(),
                    0,
                    ptr::null(),
                    1,
                    &restriction,
                    &mut restricted
                ),
                0
            );
        }
        let restricted = Handle(restricted);
        // SAFETY: there is no await or panic-prone operation between successful
        // impersonation and RevertToSelf. Assertions happen after reverting.
        let denied = unsafe {
            if ImpersonateLoggedOnUser(restricted.0) == 0 {
                false
            } else {
                let result = tokio::net::windows::named_pipe::ClientOptions::new().open(&name);
                if RevertToSelf() == 0 {
                    std::process::abort();
                }
                result.is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
            }
        };
        assert!(
            denied,
            "a restricted foreign SID was admitted by the pipe DACL"
        );
        assert!(
            crate::ipc::connect(Path::new(r"\\remote\pipe\kwel-test"))
                .await
                .is_err()
        );
    }
}
