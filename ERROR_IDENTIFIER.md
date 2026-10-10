# Error identifiers

<!-- Generated from `crates/core/src/codes.rs`: do not edit by hand.
Regenerate with `UPDATE_ERROR_DOC=1 cargo test -p oneshot-core error_document`. -->

Every error Flick shows ends with a code such as `FLK-NET-001`. Quote the code to find the line below:
what it means, and what can be done about it. The first part of the code is the area.

## General (`FLK-GEN-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-GEN-000` | Something unexpected went wrong and no more specific code applies. | Try again. If it keeps happening, report the code with the log (Settings › Advanced). |
| `FLK-GEN-001` | Flick was asked something that does not make sense (a missing or malformed value). | Check what you entered and try again. |

## Network and servers' answers (`FLK-NET-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-NET-000` | A network problem with no more specific cause. | Check your connection and try again. |
| `FLK-NET-001` | The server could not be reached (nothing answered at that address). | Check the server is on and the address is right; make sure this computer is on the same network or has internet. |
| `FLK-NET-002` | The server took too long to answer. | Try again; if the server is slow or far away, raise the request timeout in Settings › Network. |
| `FLK-NET-003` | The secure connection (HTTPS certificate) could not be trusted. | Check the address uses the right host name. For a server with a self-signed certificate, allow it in Settings › Network. |
| `FLK-NET-004` | The server answered with an internal error (HTTP 5xx). | Wait a moment and try again; check the server's own logs if it persists. |
| `FLK-NET-005` | The server answered with something Flick does not understand. | Make sure the server is up to date and is the kind (Jellyfin, Plex) you added. |
| `FLK-NET-006` | The proxy address in the settings is not valid. | Fix or clear the proxy in Settings › Network. |
| `FLK-NET-007` | Flick could not set up its network connection with the current settings. | Review Settings › Network, or reset them. |
| `FLK-NET-008` | A server address could not be built from what was given. | Check the address of the server. |

## Sign-in and permissions (`FLK-AUTH-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-AUTH-001` | The server refused the saved credentials (wrong password, revoked or expired session). | Sign in to the server again from Settings › Servers. |
| `FLK-AUTH-002` | The server knows you but does not allow this action. | Ask the server's owner for the permission, or use another account. |
| `FLK-AUTH-003` | Quick Connect is turned off on the Jellyfin server. | Sign in with a user name and password, or enable Quick Connect in the Jellyfin dashboard. |
| `FLK-AUTH-004` | The server does not allow this account to play media. | Ask the server's owner to enable playback for the account. |
| `FLK-AUTH-005` | A Plex account sign-in is needed for this and none is saved. | Sign in to plex.tv from Settings › Servers. |
| `FLK-AUTH-006` | The account is not an administrator of this server. | Use an administrator account for these settings. |
| `FLK-AUTH-007` | The AirPlay device asks for a PIN (or no longer recognises this computer). | Enter the code shown on the TV; it is asked once, then remembered. |
| `FLK-AUTH-008` | The PIN typed is not the one the AirPlay device shows. | Read the code on the TV again and retry. |

## Servers and libraries (`FLK-SRV-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-SRV-000` | What was asked for is not on the server (deleted, moved, or an id that is no longer valid). | Refresh the library; if it is gone from the server, there is nothing to do. |
| `FLK-SRV-001` | The server (or this kind of server) does not offer that feature. | — |
| `FLK-SRV-002` | The address answered, but it is not a Jellyfin server. | Check the address and port; for a reverse proxy, include its path. |
| `FLK-SRV-003` | The server is in the list but not connected right now. | Open Settings › Servers and reconnect it. |
| `FLK-SRV-004` | An item was asked from a server it does not belong to. | Reopen the title from its own server's library. |
| `FLK-SRV-005` | None of the Plex server's addresses can be reached from this network. | Check the server is on and remote access is enabled, or join its network. |
| `FLK-SRV-006` | The TMDB key is not a valid v3 key or v4 token. | Copy the key again from your TMDB account settings. |
| `FLK-SRV-007` | A library to browse was not given. | — |
| `FLK-SRV-008` | The Plex library section id is not valid. | — |
| `FLK-SRV-009` | The Plex item does not exist (any more). | Refresh the library. |
| `FLK-SRV-010` | The saved plex.tv sign-in belongs to another account than this server's. | Sign in to plex.tv again with the right account. |
| `FLK-SRV-012` | The title is not in the Plex catalogue, so it cannot be favourited or watchlisted. | — |

## Playback decisions (`FLK-PLAY-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-PLAY-000` | Playback could not start for a reason with no more specific code. | Try again, or try another version of the title. |
| `FLK-PLAY-001` | The server returned no playable version of the title. | Check the file exists and plays on the server; ask the server to rescan. |
| `FLK-PLAY-002` | The server refused to start the playback. | Check the server's playback settings and the account's rights. |
| `FLK-PLAY-003` | The server offered no stream address for the title. | Allow transcoding or Direct Stream in Settings › Playback, then retry. |
| `FLK-PLAY-004` | The Plex media has no playable file part. | Rescan the library on the Plex server. |
| `FLK-PLAY-005` | Flick found no way to play this file on this computer with the current settings. | The message lists why; allow transcoding in Settings › Playback or pick another version. |
| `FLK-PLAY-006` | An action needed something to be playing and nothing was. | — |
| `FLK-PLAY-007` | The chosen version of the title is not on the server any more. | Reopen the title and choose a version again. |

## Player engine (`FLK-PLR-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-PLR-001` | The video engine (libmpv) was not found or could not be loaded. | Reinstall Flick; if you build it yourself, put libmpv next to the app. |
| `FLK-PLR-002` | The video engine failed to start. | Restart Flick; check the log for the engine's reason. |
| `FLK-PLR-003` | The video engine is not running. | Restart Flick. |
| `FLK-PLR-004` | The video engine rejected a command or could not open the file. | Try again; the message gives the engine's reason (file unreadable, format unsupported…). |
| `FLK-PLR-005` | The libmpv on this computer is too old or too new for Flick. | Use the libmpv shipped with Flick. |
| `FLK-PLR-006` | The video engine could not play the file (unsupported or damaged). | Try another version of the title; enable server transcoding in Settings › Playback. |

## Downloads and offline (`FLK-DL-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-DL-001` | Downloads need a Flick Server invitation link and none is saved. | Add the link in Settings › Flick Server. |
| `FLK-DL-002` | Only movies and episodes can be downloaded. | — |
| `FLK-DL-003` | There was nothing to download in the chosen item. | — |
| `FLK-DL-004` | Flick cannot write the downloaded file (disk full, folder missing or not allowed). | Free some space or fix the folder's permissions, then resume the download. |
| `FLK-DL-005` | The key that protects downloaded files cannot be read from the credential store. | Unlock the keychain/credential manager and retry. If it was erased, downloads made before cannot be played: download them again. |
| `FLK-DL-006` | The download is no longer on this computer. | Download it again. |
| `FLK-DL-007` | The Flick Server refused the download. | Check the invitation link and the server's download settings. |
| `FLK-DL-008` | The connection to the Flick Server keeps failing; the download is paused. | Check your connection and resume. |
| `FLK-DL-009` | The Flick Server did not accept this device. | Add the invitation link again in Settings › Flick Server. |
| `FLK-DL-010` | The Flick Server asked to slow down. | Wait; the download retries by itself. |
| `FLK-DL-011` | The media server behind the Flick Server did not answer. | Check the Jellyfin/Plex server is running. |
| `FLK-DL-012` | The Flick Server had a problem. | Wait and resume. |
| `FLK-DL-013` | The saved details of the download are missing. | Remove the download and download it again. |
| `FLK-DL-014` | The downloaded file's path cannot be opened. | Remove the download and download it again. |
| `FLK-DL-015` | Flick could not open its local reader for protected downloads. | Restart Flick; check that no security software blocks local connections. |
| `FLK-DL-016` | The file on the server changed or arrived with the wrong size; the download restarts. | — |
| `FLK-DL-017` | The downloads folder could not be opened. | — |
| `FLK-DL-018` | A download id is not valid. | — |
| `FLK-DL-019` | The server of this title is no longer in Flick. | Add the server again in Settings › Servers. |

## Watch together (`FLK-SYNC-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-SYNC-000` | Something went wrong with the watch room. | Leave and rejoin the room. |
| `FLK-SYNC-001` | Only the host can choose what the room watches. | Ask the host. |
| `FLK-SYNC-002` | Only the host can control playback in this room. | Ask the host to allow guests to control playback. |
| `FLK-SYNC-003` | The room does not exist (any more). | Ask for a new code. |
| `FLK-SYNC-004` | The room is full. | Ask the host to make room. |
| `FLK-SYNC-005` | The host closed the room. | — |
| `FLK-SYNC-006` | The host picked something Flick cannot share. | Pick a movie or an episode. |
| `FLK-SYNC-007` | The sign-in to the FlickSync server expired. | Try again; check the Flick Server link in Settings. |
| `FLK-SYNC-008` | Too many requests in a short time. | Wait a few seconds. |
| `FLK-SYNC-009` | Chat is turned off in this room. | — |
| `FLK-SYNC-010` | This Flick version and the FlickSync server do not speak the same protocol version. | Update Flick or the server. |
| `FLK-SYNC-011` | Watch together is not reachable right now. | Check the Flick Server is up and your connection works. |
| `FLK-SYNC-012` | The title is not available on your connected server. | Add the server that has it, or watch something else. |
| `FLK-SYNC-013` | Watch together is not set up. | Add the Flick Server link in Settings › Flick Server. |
| `FLK-SYNC-014` | An action needed a room and you are not in one. | Join or create a room. |
| `FLK-SYNC-015` | Only movies and episodes can be watched together. | — |
| `FLK-SYNC-017` | The connection to the room was lost. | Rejoin the room. |

## Flick Server invitation links (`FLK-LINK-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-LINK-001` | The text is not a Flick Server invitation link. | Copy the link from the Flick Server again. |
| `FLK-LINK-002` | The link comes from a newer Flick Server. | Update Flick. |
| `FLK-LINK-003` | The link has no key or its key is unreadable (cut when copying). | Copy the whole link again. |
| `FLK-LINK-004` | The link's address, path, version or TLS part is damaged. | Copy it again in full, or ask for a new one. |
| `FLK-LINK-006` | The server did not answer, so the link was not saved. | Check the link, that the server is on, and try again. |

## Casting (`FLK-CAST-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-CAST-001` | The cast device is no longer on the network. | Check it is on, then pick it again. |
| `FLK-CAST-002` | Nothing is being cast. | — |
| `FLK-CAST-003` | The Chromecast did not answer or closed the connection. | Check it is on the same network, restart it, and retry. |
| `FLK-CAST-004` | The Chromecast could not load the stream. | Try another version of the title, or stream from the server with transcoding allowed. |
| `FLK-CAST-005` | The AirPlay device refused or could not play the stream. | Check the device accepts streams from this network. |
| `FLK-CAST-006` | The device's address is not valid. | Refresh the device list. |
| `FLK-CAST-007` | Flick could not open the local relay the device pulls the video from. | Check no firewall blocks Flick on the local network. |
| `FLK-CAST-008` | The cast device sent something malformed. | Restart the device. |
| `FLK-CAST-010` | The AirPlay device is busy or asks to wait before another PIN attempt. | Wait a minute and retry. |
| `FLK-CAST-011` | The pairing with the AirPlay device failed or could not be verified. | Retry; if it persists, remove the device's pairing in its settings (Remotes and Devices) and pair again. |
| `FLK-CAST-012` | AirPlay cannot play this title: it has no video Flick can convert. | Watch it in Flick, or cast it to a Chromecast. |
| `FLK-CAST-013` | The AirPlay device could not be reached, or stopped answering. | Check it is on (wake it up) and on the same network as this computer, not a guest Wi-Fi, then retry. |
| `FLK-CAST-014` | The encrypted connection with the AirPlay device broke or could not be checked. | Retry. If it keeps happening, remove Flick from the device's paired remotes and pair again. |
| `FLK-CAST-015` | ffmpeg, which converts videos for AirPlay, was not found. | Reinstall Flick; when building it yourself, run `node tools/ensure-libmpv.mjs`. |
| `FLK-CAST-016` | The conversion of the video for AirPlay stopped unexpectedly. | Retry; if it persists, try another version of the title (the log has ffmpeg's reason). |
| `FLK-CAST-017` | The conversion of the video for AirPlay did not start in time. | Try again, or watch it in Flick. A very large or damaged file can take too long to open. |
| `FLK-CAST-009` | A casting problem with no more specific cause. | Restart the device and retry. |

## Profiles and PINs (`FLK-PROF-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-PROF-001` | The PIN is wrong. | Try again. |
| `FLK-PROF-002` | Too many wrong PINs: PIN entry is locked for a short time. | Wait for the delay shown. |
| `FLK-PROF-003` | A PIN must be exactly 4 digits. | Use 4 digits. |
| `FLK-PROF-004` | The profile does not exist (any more). | Reopen the profile list. |
| `FLK-PROF-005` | A profile switch is already in progress. | Wait for it to finish. |
| `FLK-PROF-006` | A profile needs a name. | Type a name. |
| `FLK-PROF-007` | That account is not part of this profile. | — |
| `FLK-PROF-008` | A profile cannot be merged with itself. | — |
| `FLK-PROF-009` | Only profiles made from server users can be merged. | — |
| `FLK-PROF-010` | Profiles made from server users are hidden, not deleted. | Hide it instead. |
| `FLK-PROF-012` | Flick could not protect the PIN. | Try again. |

## Storage and credentials (`FLK-STO-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-STO-000` | A file could not be read or written. | Check the disk has space and Flick may write to its folder. |
| `FLK-STO-001` | The system credential store (Keychain, Credential Manager) is not available. | Unlock it or allow Flick access, then retry. Flick never keeps passwords in plain files. |
| `FLK-STO-002` | The saved credentials cannot be read back. | Sign in to your servers again. |
| `FLK-STO-003` | A credential could not be saved. | Unlock the credential store and retry. |
| `FLK-STO-004` | A settings or data file could not be read or written. | Check the disk space and the folder's permissions. |
| `FLK-STO-005` | The metadata cache could not be written. | Clear the cache in Settings › Network & Cache. |
| `FLK-STO-006` | The picture cache could not be written or cleared. | Check the disk space. |
| `FLK-STO-007` | The system could not provide a random key. | Restart the computer. |

## Pictures (`FLK-IMG-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-IMG-001` | An avatar address is malformed. | — |
| `FLK-IMG-002` | The profile's picture is missing. | Choose a picture again. |
| `FLK-IMG-003` | A TMDB picture address is malformed. | — |
| `FLK-IMG-004` | A picture could not be decoded. | — |
| `FLK-IMG-005` | A picture of a download is not stored. | — |

## Updates (`FLK-UPD-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-UPD-001` | The update server could not be reached. | Check your connection. |
| `FLK-UPD-002` | No published release was found to update from. | Try later. |
| `FLK-UPD-003` | Updates are not installed in a development build. | — |
| `FLK-UPD-004` | There is no update waiting to be installed. | Check for updates again. |
| `FLK-UPD-005` | The update failed (download, signature or installation). | Try again; download the new version manually if it persists. |

## Window and system (`FLK-SYS-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-SYS-001` | The window could not do what was asked (fullscreen, picture-in-picture…). | Try again. |
| `FLK-SYS-002` | A background task stopped unexpectedly. | Try again; restart Flick if it persists. |

## Screens (`FLK-UI-…`)

| Code | Meaning | What to do |
|---|---|---|
| `FLK-UI-001` | A screen of Flick crashed and was replaced by an error page. | Use Reload; report the code with the technical text on the page. |
| `FLK-UI-002` | The text could not be copied to the clipboard. | Select and copy it by hand. |

