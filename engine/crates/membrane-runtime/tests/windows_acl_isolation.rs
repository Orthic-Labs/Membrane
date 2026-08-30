//! Windows-only qualification of the local trust surface ACLs.

#[cfg(windows)]
mod windows_acl {
    use membrane_runtime::native_diagnostics_pipe::{canonical_pipe_name, start_resident};
    use membrane_runtime::serve::configured_api_token;
    use membrane_runtime::service::{install_lifecycle_control, LifecycleControl};
    use membrane_runtime::DiagnosticsService;
    use serde_json::json;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::ptr::null_mut;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    type Handle = *mut c_void;
    type Sid = *mut c_void;
    type Acl = c_void;
    type SecurityDescriptor = c_void;

    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const SE_FILE_OBJECT: u32 = 1;
    const SE_KERNEL_OBJECT: u32 = 6;
    const OWNER_SECURITY_INFORMATION: u32 = 0x0000_0001;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const SE_DACL_PROTECTED: u16 = 0x1000;
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const GENERIC_ALL: u32 = 0x1000_0000;

    #[repr(C)]
    struct AclSizeInformation {
        ace_count: u32,
        acl_bytes_in_use: u32,
        acl_bytes_free: u32,
    }

    #[repr(C)]
    struct AceHeader {
        ace_type: u8,
        ace_flags: u8,
        ace_size: u16,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share_mode: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn GetSecurityInfo(
            handle: Handle,
            object_type: u32,
            security_info: u32,
            owner: *mut Sid,
            group: *mut Sid,
            dacl: *mut *mut Acl,
            sacl: *mut *mut Acl,
            descriptor: *mut *mut SecurityDescriptor,
        ) -> u32;
        fn GetSecurityDescriptorDacl(
            descriptor: *mut SecurityDescriptor,
            dacl_present: *mut i32,
            dacl: *mut *mut Acl,
            dacl_defaulted: *mut i32,
        ) -> i32;
        fn GetSecurityDescriptorControl(
            descriptor: *mut SecurityDescriptor,
            control: *mut u16,
            revision: *mut u32,
        ) -> i32;
        fn GetAclInformation(
            acl: *mut Acl,
            information: *mut c_void,
            length: u32,
            information_class: u32,
        ) -> i32;
        fn GetAce(acl: *mut Acl, index: u32, ace: *mut *mut c_void) -> i32;
        fn EqualSid(left: Sid, right: Sid) -> i32;
    }

    fn assert_owner_only(handle: Handle, object_type: u32, object_name: &str) {
        let mut owner: Sid = null_mut();
        let mut group: Sid = null_mut();
        let mut dacl: *mut Acl = null_mut();
        let mut descriptor: *mut SecurityDescriptor = null_mut();
        let result = unsafe {
            GetSecurityInfo(
                handle,
                object_type,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                &mut group,
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(result, 0, "GetSecurityInfo({object_name}) failed: {result}");
        assert!(!owner.is_null(), "{object_name} has no owner SID");
        assert!(!descriptor.is_null(), "{object_name} has no security descriptor");

        let mut control = 0u16;
        let mut revision = 0u32;
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0,
            "{object_name} security descriptor control unavailable"
        );
        assert_ne!(
            control & SE_DACL_PROTECTED,
            0,
            "{object_name} DACL inheritance is not disabled"
        );

        let mut dacl_present = 0i32;
        let mut descriptor_dacl: *mut Acl = null_mut();
        let mut dacl_defaulted = 0i32;
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(
                    descriptor,
                    &mut dacl_present,
                    &mut descriptor_dacl,
                    &mut dacl_defaulted,
                )
            },
            0,
            "{object_name} DACL unavailable"
        );
        assert_ne!(dacl_present, 0, "{object_name} has no DACL");
        assert!(!descriptor_dacl.is_null(), "{object_name} has a null DACL");

        let mut size = AclSizeInformation {
            ace_count: 0,
            acl_bytes_in_use: 0,
            acl_bytes_free: 0,
        };
        assert_ne!(
            unsafe {
                GetAclInformation(
                    descriptor_dacl,
                    &mut size as *mut AclSizeInformation as *mut c_void,
                    std::mem::size_of::<AclSizeInformation>() as u32,
                    2, // AclSizeInformation
                )
            },
            0,
            "{object_name} ACL size unavailable"
        );
        assert_eq!(size.ace_count, 1, "{object_name} grants more than its owner");

        let mut ace: *mut c_void = null_mut();
        assert_ne!(
            unsafe { GetAce(descriptor_dacl, 0, &mut ace) },
            0,
            "{object_name} owner ACE unavailable"
        );
        assert!(!ace.is_null(), "{object_name} has a null ACE");
        let header = unsafe { &*(ace as *const AceHeader) };
        assert_eq!(header.ace_type, ACCESS_ALLOWED_ACE_TYPE);
        assert_eq!(header.ace_flags, 0, "{object_name} owner ACE is inherited");
        let mask = unsafe { *((ace as *const u8).add(4) as *const u32) };
        assert_eq!(mask, GENERIC_ALL, "{object_name} owner ACE is not explicit full owner access");
        let ace_sid = unsafe { (ace as *mut u8).add(8) as Sid };
        assert_ne!(
            unsafe { EqualSid(owner, ace_sid) },
            0,
            "{object_name} ACE does not name the owning user"
        );

        unsafe { LocalFree(descriptor) };
    }

    fn open_pipe(name: &str) -> Option<Handle> {
        let name = std::ffi::OsStr::new(name)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null_mut(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        (handle != INVALID_HANDLE_VALUE).then_some(handle)
    }

    #[test]
    fn bearer_file_and_named_pipe_are_owner_only_and_non_inheriting() {
        let temp = tempfile::tempdir().expect("temporary ACL qualification directory");
        let token_path = temp.path().join("api-token");
        std::env::remove_var("MEMBRANE_API_TOKEN");
        std::env::set_var("MEMBRANE_API_TOKEN_FILE", &token_path);
        let token = configured_api_token(&temp.path().join("cortex.db"))
            .expect("owner-only bearer file creation");
        assert!(!token.is_empty());
        let token_file = std::fs::File::open(&token_path).expect("open bearer file");
        assert_owner_only(
            token_file.as_raw_handle() as Handle,
            SE_FILE_OBJECT,
            "api-token bearer file",
        );
        std::env::remove_var("MEMBRANE_API_TOKEN_FILE");

        let lifecycle = LifecycleControl::default();
        install_lifecycle_control(lifecycle.clone()).expect("install test lifecycle");
        let service = DiagnosticsService::with_data_root(temp.path().to_path_buf())
            .expect("diagnostics service");
        start_resident(
            Arc::new(Mutex::new(service)),
            json!({
                "installationId": "acl-test-installation",
                "cortexStoreId": "acl-test-store",
                "releaseGeneration": "acl-test-release",
                "serviceGeneration": "acl-test-service"
            }),
        );
        let name = canonical_pipe_name();
        let deadline = Instant::now() + Duration::from_secs(5);
        let pipe = loop {
            if let Some(handle) = open_pipe(&name) {
                break handle;
            }
            assert!(Instant::now() < deadline, "named pipe was not created: {name}");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_owner_only(pipe, SE_KERNEL_OBJECT, "per-user diagnostics named pipe");
        unsafe { CloseHandle(pipe) };
        lifecycle.request_drain(Some("windows-acl-qualification-complete"));
    }
}

#[cfg(not(windows))]
#[test]
fn windows_acl_isolation_is_typed_unavailable_off_windows() {
    eprintln!(
        "SKIPPED: unavailable {{ reason: host_unsupported, detail: Windows ACL qualification is Windows-only }}"
    );
}
