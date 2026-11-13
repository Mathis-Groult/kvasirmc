//! The Bedrock disconnect reason a plugin passes, as the engine's own enum.

use crate::pumpkin::plugin::player::BedrockDisconnectReason as Wasm;
use pumpkin_core::net::DisconnectReason;

/// Maps a plugin's Bedrock disconnect reason to the engine's.
#[allow(clippy::too_many_lines)]
pub(super) const fn from_wasm_bedrock_disconnect_reason(reason: Wasm) -> DisconnectReason {
    match reason {
        Wasm::Unknown => DisconnectReason::Unknown,
        Wasm::CantConnectNoInternet => DisconnectReason::CantConnectNoInternet,
        Wasm::NoPermissions => DisconnectReason::NoPermissions,
        Wasm::UnrecoverableError => DisconnectReason::UnrecoverableError,
        Wasm::ThirdPartyBlocked => DisconnectReason::ThirdPartyBlocked,
        Wasm::ThirdPartyNoInternet => DisconnectReason::ThirdPartyNoInternet,
        Wasm::ThirdPartyBadIp => DisconnectReason::ThirdPartyBadIP,
        Wasm::ThirdPartyNoServerOrServerLocked => {
            DisconnectReason::ThirdPartyNoServerOrServerLocked
        }
        Wasm::VersionMismatch => DisconnectReason::VersionMismatch,
        Wasm::SkinIssue => DisconnectReason::SkinIssue,
        Wasm::InviteSessionNotFound => DisconnectReason::InviteSessionNotFound,
        Wasm::EduLevelSettingsMissing => DisconnectReason::EduLevelSettingsMissing,
        Wasm::LocalServerNotFound => DisconnectReason::LocalServerNotFound,
        Wasm::LegacyDisconnect => DisconnectReason::LegacyDisconnect,
        Wasm::UserLeaveGameAttempted => DisconnectReason::UserLeaveGameAttempted,
        Wasm::PlatformLockedSkinsError => DisconnectReason::PlatformLockedSkinsError,
        Wasm::RealmsWorldUnassigned => DisconnectReason::RealmsWorldUnassigned,
        Wasm::RealmsServerCantConnect => DisconnectReason::RealmsServerCantConnect,
        Wasm::RealmsServerHidden => DisconnectReason::RealmsServerHidden,
        Wasm::RealmsServerDisabledBeta => DisconnectReason::RealmsServerDisabledBeta,
        Wasm::RealmsServerDisabled => DisconnectReason::RealmsServerDisabled,
        Wasm::CrossPlatformDisabled => DisconnectReason::CrossPlatformDisabled,
        Wasm::CantConnect => DisconnectReason::CantConnect,
        Wasm::SessionNotFound => DisconnectReason::SessionNotFound,
        Wasm::ClientSettingsIncompatibleWithServer => {
            DisconnectReason::ClientSettingsIncompatibleWithServer
        }
        Wasm::ServerFull => DisconnectReason::ServerFull,
        Wasm::InvalidPlatformSkin => DisconnectReason::InvalidPlatformSkin,
        Wasm::EditionVersionMismatch => DisconnectReason::EditionVersionMismatch,
        Wasm::EditionMismatch => DisconnectReason::EditionMismatch,
        Wasm::LevelNewerThanExeVersion => DisconnectReason::LevelNewerThanExeVersion,
        Wasm::NoFailOccurred => DisconnectReason::NoFailOccurred,
        Wasm::BannedSkin => DisconnectReason::BannedSkin,
        Wasm::Timeout => DisconnectReason::Timeout,
        Wasm::ServerNotFound => DisconnectReason::ServerNotFound,
        Wasm::OutdatedServer => DisconnectReason::OutdatedServer,
        Wasm::OutdatedClient => DisconnectReason::OutdatedClient,
        Wasm::NoPremiumPlatform => DisconnectReason::NoPremiumPlatform,
        Wasm::MultiplayerDisabled => DisconnectReason::MultiplayerDisabled,
        Wasm::NoWifi => DisconnectReason::NoWiFi,
        Wasm::WorldCorruption => DisconnectReason::WorldCorruption,
        Wasm::NoReason => DisconnectReason::NoReason,
        Wasm::Disconnected => DisconnectReason::Disconnected,
        Wasm::InvalidPlayer => DisconnectReason::InvalidPlayer,
        Wasm::LoggedInOtherLocation => DisconnectReason::LoggedInOtherLocation,
        Wasm::ServerIdConflict => DisconnectReason::ServerIdConflict,
        Wasm::NotAllowed => DisconnectReason::NotAllowed,
        Wasm::NotAuthenticated => DisconnectReason::NotAuthenticated,
        Wasm::InvalidTenant => DisconnectReason::InvalidTenant,
        Wasm::UnknownPacket => DisconnectReason::UnknownPacket,
        Wasm::UnexpectedPacket => DisconnectReason::UnexpectedPacket,
        Wasm::InvalidCommandRequestPacket => DisconnectReason::InvalidCommandRequestPacket,
        Wasm::HostSuspended => DisconnectReason::HostSuspended,
        Wasm::LoginPacketNoRequest => DisconnectReason::LoginPacketNoRequest,
        Wasm::LoginPacketNoCert => DisconnectReason::LoginPacketNoCert,
        Wasm::MissingClient => DisconnectReason::MissingClient,
        Wasm::Kicked => DisconnectReason::Kicked,
        Wasm::KickedForExploit => DisconnectReason::KickedForExploit,
        Wasm::KickedForIdle => DisconnectReason::KickedForIdle,
        Wasm::ResourcePackProblem => DisconnectReason::ResourcePackProblem,
        Wasm::IncompatiblePack => DisconnectReason::IncompatiblePack,
        Wasm::OutOfStorage => DisconnectReason::OutOfStorage,
        Wasm::InvalidLevel => DisconnectReason::InvalidLevel,
        Wasm::DisconnectPacket => DisconnectReason::DisconnectPacket,
        Wasm::BlockMismatch => DisconnectReason::BlockMismatch,
        Wasm::InvalidHeights => DisconnectReason::InvalidHeights,
        Wasm::InvalidWidths => DisconnectReason::InvalidWidths,
        Wasm::ConnectionLost => DisconnectReason::ConnectionLost,
        Wasm::ZombieConnection => DisconnectReason::ZombieConnection,
        Wasm::Shutdown => DisconnectReason::Shutdown,
        Wasm::ReasonNotSet => DisconnectReason::ReasonNotSet,
        Wasm::LoadingStateTimeout => DisconnectReason::LoadingStateTimeout,
        Wasm::ResourcePackLoadingFailed => DisconnectReason::ResourcePackLoadingFailed,
        Wasm::SearchingForSessionLoadingScreenFailed => {
            DisconnectReason::SearchingForSessionLoadingScreenFailed
        }
        Wasm::NetherNetProtocolVersion => DisconnectReason::NetherNetProtocolVersion,
        Wasm::SubsystemStatusError => DisconnectReason::SubsystemStatusError,
        Wasm::EmptyAuthFromDiscovery => DisconnectReason::EmptyAuthFromDiscovery,
        Wasm::EmptyUrlFromDiscovery => DisconnectReason::EmptyUrlFromDiscovery,
        Wasm::ExpiredAuthFromDiscovery => DisconnectReason::ExpiredAuthFromDiscovery,
        Wasm::UnknownSignalServiceSignInFailure => {
            DisconnectReason::UnknownSignalServiceSignInFailure
        }
        Wasm::XblJoinLobbyFailure => DisconnectReason::XBLJoinLobbyFailure,
        Wasm::UnspecifiedClientInstanceDisconnection => {
            DisconnectReason::UnspecifiedClientInstanceDisconnection
        }
        Wasm::NetherNetSessionNotFound => DisconnectReason::NetherNetSessionNotFound,
        Wasm::NetherNetCreatePeerConnection => DisconnectReason::NetherNetCreatePeerConnection,
        Wasm::NetherNetIce => DisconnectReason::NetherNetICE,
        Wasm::NetherNetConnectRequest => DisconnectReason::NetherNetConnectRequest,
        Wasm::NetherNetConnectResponse => DisconnectReason::NetherNetConnectResponse,
        Wasm::NetherNetNegotiationTimeout => DisconnectReason::NetherNetNegotiationTimeout,
        Wasm::NetherNetInactivityTimeout => DisconnectReason::NetherNetInactivityTimeout,
        Wasm::StaleConnectionBeingReplaced => DisconnectReason::StaleConnectionBeingReplaced,
        Wasm::RealmsSessionNotFound => DisconnectReason::RealmsSessionNotFound,
        Wasm::BadPacket => DisconnectReason::BadPacket,
        Wasm::NetherNetFailedToCreateOffer => DisconnectReason::NetherNetFailedToCreateOffer,
        Wasm::NetherNetFailedToCreateAnswer => DisconnectReason::NetherNetFailedToCreateAnswer,
        Wasm::NetherNetFailedToSetLocalDescription => {
            DisconnectReason::NetherNetFailedToSetLocalDescription
        }
        Wasm::NetherNetFailedToSetRemoteDescription => {
            DisconnectReason::NetherNetFailedToSetRemoteDescription
        }
        Wasm::NetherNetNegotiationTimeoutWaitingForResponse => {
            DisconnectReason::NetherNetNegotiationTimeoutWaitingForResponse
        }
        Wasm::NetherNetNegotiationTimeoutWaitingForAccept => {
            DisconnectReason::NetherNetNegotiationTimeoutWaitingForAccept
        }
        Wasm::NetherNetIncomingConnectionIgnored => {
            DisconnectReason::NetherNetIncomingConnectionIgnored
        }
        Wasm::NetherNetSignalingParsingFailure => {
            DisconnectReason::NetherNetSignalingParsingFailure
        }
        Wasm::NetherNetSignalingUnknownError => DisconnectReason::NetherNetSignalingUnknownError,
        Wasm::NetherNetSignalingUnicastDeliveryFailed => {
            DisconnectReason::NetherNetSignalingUnicastDeliveryFailed
        }
        Wasm::NetherNetSignalingBroadcastDeliveryFailed => {
            DisconnectReason::NetherNetSignalingBroadcastDeliveryFailed
        }
        Wasm::NetherNetSignalingGenericDeliveryFailed => {
            DisconnectReason::NetherNetSignalingGenericDeliveryFailed
        }
        Wasm::EditorMismatchEditorWorld => DisconnectReason::EditorMismatchEditorWorld,
        Wasm::EditorMismatchVanillaWorld => DisconnectReason::EditorMismatchVanillaWorld,
        Wasm::WorldTransferNotPrimaryClient => DisconnectReason::WorldTransferNotPrimaryClient,
        Wasm::RequestServerShutdown => DisconnectReason::RequestServerShutdown,
        Wasm::ClientGameSetupCancelled => DisconnectReason::ClientGameSetupCancelled,
        Wasm::ClientGameSetupFailed => DisconnectReason::ClientGameSetupFailed,
        Wasm::NoVenue => DisconnectReason::NoVenue,
        Wasm::NetherNetSignalingSigninFailed => DisconnectReason::NetherNetSignalingSigninFailed,
        Wasm::SessionAccessDenied => DisconnectReason::SessionAccessDenied,
        Wasm::ServiceSigninIssue => DisconnectReason::ServiceSigninIssue,
        Wasm::NetherNetNoSignalingChannel => DisconnectReason::NetherNetNoSignalingChannel,
        Wasm::NetherNetNotLoggedIn => DisconnectReason::NetherNetNotLoggedIn,
        Wasm::NetherNetClientSignalingError => DisconnectReason::NetherNetClientSignalingError,
        Wasm::SubClientLoginDisabled => DisconnectReason::SubClientLoginDisabled,
        Wasm::DeepLinkTryingToOpenDemoWorldWhileSignedIn => {
            DisconnectReason::DeepLinkTryingToOpenDemoWorldWhileSignedIn
        }
        Wasm::AsyncJoinTaskDenied => DisconnectReason::AsyncJoinTaskDenied,
        Wasm::RealmsTimelineRequired => DisconnectReason::RealmsTimelineRequired,
        Wasm::GuestWithoutHost => DisconnectReason::GuestWithoutHost,
        Wasm::FailedToJoinExperience => DisconnectReason::FailedToJoinExperience,
        Wasm::NetherNetDataChannelClosed => DisconnectReason::NetherNetDataChannelClosed,
        Wasm::DiscoveryEnvironmentMismatch => DisconnectReason::DiscoveryEnvironmentMismatch,
        Wasm::HostWithoutKeys => DisconnectReason::HostWithoutKeys,
        Wasm::HostSignedOut => DisconnectReason::HostSignedOut,
        Wasm::ScriptWatchdogException => DisconnectReason::ScriptWatchdogException,
        Wasm::ScriptMemoryLimitExceeded => DisconnectReason::ScriptMemoryLimitExceeded,
        Wasm::StorageLowDuringGameplay => DisconnectReason::StorageLowDuringGameplay,
        Wasm::StorageFullDuringGameplay => DisconnectReason::StorageFullDuringGameplay,
        Wasm::LevelStorageCorruption => DisconnectReason::LevelStorageCorruption,
        Wasm::EditionMismatchVanillaToEdu => DisconnectReason::EditionMismatchVanillaToEdu,
        Wasm::EditionMismatchEduToVanilla => DisconnectReason::EditionMismatchEduToVanilla,
        Wasm::EditorMismatchEditorToVanilla => DisconnectReason::EditorMismatchEditorToVanilla,
        Wasm::EditorMismatchVanillaToEditor => DisconnectReason::EditorMismatchVanillaToEditor,
        Wasm::DenyListed => DisconnectReason::DenyListed,
        Wasm::NonceMissing => DisconnectReason::NonceMissing,
        Wasm::NonceNotFound => DisconnectReason::NonceNotFound,
        Wasm::NonceExpired => DisconnectReason::NonceExpired,
        Wasm::NonceNotValid => DisconnectReason::NonceNotValid,
        Wasm::HostDisconnected => DisconnectReason::HostDisconnected,
        Wasm::EditorJoinIntentPolicyFailure => DisconnectReason::EditorJoinIntentPolicyFailure,
        Wasm::NetherNetIdentityNotAllowed => DisconnectReason::NetherNetIdentityNotAllowed,
        Wasm::InvalidName => DisconnectReason::InvalidName,
        Wasm::ExpiredToken => DisconnectReason::ExpiredToken,
        Wasm::HostAcceptsNoTypeOfAuth => DisconnectReason::HostAcceptsNoTypeOfAuth,
        Wasm::NotAuthenticatedFastFail => DisconnectReason::NotAuthenticatedFastFail,
        Wasm::EditorNotAllowed => DisconnectReason::EditorNotAllowed,
    }
}
