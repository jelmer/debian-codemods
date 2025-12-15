//! Systemd-specific metadata and domain knowledge
//!
//! This module contains information about systemd directives, their types,
//! and how they should be merged in drop-in files.

/// Check if a directive is accumulating (values should be added rather than replaced)
///
/// In systemd, some directives accumulate values across drop-ins (like Wants=, After=),
/// while others replace (like Description=, Type=).
///
/// This is based on systemd's behavior where certain directives are list-based
/// and should accumulate when multiple values are specified across the main unit
/// and drop-in files.
pub fn is_accumulating_directive(key: &str) -> bool {
    matches!(
        key,
        // Unit dependencies and ordering
        "Wants"
            | "Requires"
            | "Requisite"
            | "BindsTo"
            | "PartOf"
            | "Upholds"
            | "After"
            | "Before"
            | "Conflicts"
            | "OnFailure"
            | "OnSuccess"
            | "PropagatesReloadTo"
            | "ReloadPropagatedFrom"
            | "PropagatesStopTo"
            | "StopPropagatedFrom"
            | "JoinsNamespaceOf"
            | "RequiresMountsFor"
            | "OnSuccessJobMode"
            | "OnFailureJobMode"
            // Environment
            | "Environment"
            | "EnvironmentFile"
            | "PassEnvironment"
            | "UnsetEnvironment"
            // Execution
            | "ExecStartPre"
            | "ExecStartPost"
            | "ExecCondition"
            | "ExecReload"
            | "ExecStop"
            | "ExecStopPost"
            // Groups and users
            | "SupplementaryGroups"
            // Paths and security
            | "ReadWritePaths"
            | "ReadOnlyPaths"
            | "InaccessiblePaths"
            | "ExecPaths"
            | "NoExecPaths"
            | "ExecSearchPath"
            | "LogExtraFields"
            | "RestrictAddressFamilies"
            | "SystemCallFilter"
            | "SystemCallLog"
            | "SystemCallArchitectures"
            | "RestrictNetworkInterfaces"
            | "BindPaths"
            | "BindReadOnlyPaths"
            // Device access
            | "DeviceAllow"
            // Sockets
            | "ListenStream"
            | "ListenDatagram"
            | "ListenSequentialPacket"
            | "ListenFIFO"
            | "ListenSpecial"
            | "ListenNetlink"
            | "ListenMessageQueue"
            | "ListenUSBFunction"
            // Path
            | "PathExists"
            | "PathExistsGlob"
            | "PathChanged"
            | "PathModified"
            | "DirectoryNotEmpty"
            // Timer
            | "OnActiveSec"
            | "OnBootSec"
            | "OnStartupSec"
            | "OnUnitActiveSec"
            | "OnUnitInactiveSec"
            | "OnCalendar"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accumulating_directives() {
        assert!(is_accumulating_directive("Wants"));
        assert!(is_accumulating_directive("After"));
        assert!(is_accumulating_directive("Requires"));
        assert!(is_accumulating_directive("Environment"));
    }

    #[test]
    fn test_non_accumulating_directives() {
        assert!(!is_accumulating_directive("Description"));
        assert!(!is_accumulating_directive("Type"));
        assert!(!is_accumulating_directive("ExecStart"));
        assert!(!is_accumulating_directive("User"));
    }
}
