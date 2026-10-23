# Changelog

All notable changes to Dictator. The newest version is shown in the app's What's New window.

## [Unreleased]

### Added
- A third speech model for English: Phonon-2 by Fermion Research (about 358 MB), a smaller re-training of NVIDIA Parakeet. Choose it in Settings → Model or on first launch; it downloads only when you pick it. Parakeet stays the recommended model until Phonon-2 proves better on real dictations. Phonon-2 is licensed CC-BY-4.0; the credit is in the license notices.
- Optional cleanup of each dictation with S1-mini by Superwhisper, a small model that runs on your Mac. It removes filler words like "um", applies spoken corrections ("by Friday, no wait, Thursday" becomes "by Thursday"), and formats numbers, dates, times and email addresses. Off by default: turn it on in Settings → Cleanup, which downloads the model (about 484 MB) and lets you pick a style from casual to formal. English only. If cleanup isn't ready, fails or takes too long, the dictation is typed as transcribed, and undo still removes exactly what was typed. S1-mini is licensed Apache 2.0; the credit is in the license notices.

### Fixed
- Switching speech models back and forth while one downloads no longer starts a second download of the same model.
- Settings no longer offers Delete for the model still in use while another one downloads.
- Cancelling or timing out a dictation now stops transcription the same way for both speech models.
- Dictations longer than 30 seconds no longer lose words. Long recordings are now cut at a pause, and no piece is too short to be transcribed.

### Changed
- Dictator is English-only. The Dansk language option is removed; Whisper offers Automatic or English. Parakeet remains English only.

## [0.4.0] - 2026-10-15

### Added
- A second, faster and more accurate speech model for English: NVIDIA Parakeet TDT 0.6B v2 (about 464 MB). Whisper Large v3 Turbo stays available, and is still the one to use for Dansk. It is licensed CC-BY-4.0; the credit is in the license notices.
- Dictator no longer downloads a model on its own. On first launch it asks which model to download, best first, with the size, accuracy and speed of each. Copies that already downloaded Whisper keep using it without being asked. Switch models any time in Settings → Model, which downloads the new one on demand and lets you delete the one you no longer use.

## [0.3.0] - 2026-10-14

### Added
- Choose the microphone for dictation in Settings → General. Dictation can use a dedicated microphone while calls and other apps keep using the input set in System Settings, such as a headset. If the chosen microphone is unplugged, Dictator uses the system default until it's connected again. Audio interfaces with the microphone on another input than the first now record it too.

## [0.2.0] - 2026-10-14

### Added
- Updates install from the menu. Choose Install Update and Dictator downloads the release, checks its published checksum and Apple signature, replaces itself and relaunches. Until you are on this version, update once from the release page; after that the menu does it. The daily check still downloads nothing until you choose to install, and you can turn it off.

### Fixed
- Changing the dictation shortcut to the keys of the Undo Last Dictation shortcut no longer loses the undo shortcut when the new shortcut can't be registered. Both shortcuts stay as they were, and Settings shows why the change failed.
- Recording an empty dictation shortcut now keeps the "Press at least one key." hint visible instead of clearing it right away.

## [0.1.4] - 2026-10-13

### Fixed
- Long dictations could come out with a piece of text moved to the end (for example "…logos toe can showcase … take. be on the card so w"). Dictator typed the text as a fast burst of small key events, and one could arrive after the ones behind it. It now types them one after another with a tiny pause, so the text arrives complete and in order (#43).

## [0.1.3] - 2026-10-12

### Fixed
- Settings fit on the screen. Categories sit in a sidebar, and the selected page scrolls instead of making the window taller than the display.

### Changed
- The speech model is no longer inside the app, so the download is about 1.6 GB smaller. On first launch (and after updating to this version) Dictator downloads the model once, automatically, with progress in the menu. It resumes after a quit or a dropped connection, and the menu offers Retry if it fails. Every file is checked against a SHA-256 list built into the signed app. Audio never leaves the Mac; the one-time model download is the only network use besides the optional update check.
- The update check now asks GitHub about the new home of the project, captains-chest/yet-another-dictator. Earlier versions keep working through GitHub's redirect.

## [0.1.2] - 2026-10-10

### Fixed
- After updating from 0.1.0, Input Monitoring and Accessibility could stay "not granted" even though they were switched on in System Settings. That switch belonged to the old unsigned build. Settings now explains this and opens the right page: remove Dictator from the list with −, then click Grant… again. From 0.1.1 on, updates keep these permissions.

## [0.1.1] - 2026-10-10

Signed and notarized.

### Changed
- Dictator is now signed with an Apple Developer ID and notarized by Apple. It opens without the "Open Anyway" step, and macOS keeps its Microphone, Input Monitoring and Accessibility permissions when you update. If you're coming from 0.1.0, grant them once more.
- Cancel Dictation now also stops typing a long dictation part-way. The part already typed stays, but it isn't offered for Undo.
- The dictation core now comes from the shared [DictationCore](https://github.com/captains-chest/DictationCore) package, so a future iOS app can use the same code. The app behaves the same.
- The app bundle includes its license and third-party notices.

### Fixed
- A recording that reaches the 10-minute limit now stops and is typed, with a short notice, instead of silently dropping everything said after 10 minutes.

## [0.1.0] - 2026-10-09

### Added
- Undo Last Dictation: from the menu, or with an optional shortcut set in Settings. Removes exactly what was just typed, as long as you haven't typed or clicked since.
- Smart spacing and capitals: dictations continue the sentence with the right space and capitalisation.
- Update check: once a day Dictator asks GitHub whether a newer version exists and offers the download page. Nothing is downloaded automatically, and you can turn it off in Settings.
- What's New window, shown once after an update; reopen it from the menu.
- Choose the dictation language: Automatic, Dansk or English.
- Open at login.
- Keep microphone ready (optional): catches the first word if you start speaking as you press the shortcut.
- Settings shows when the shortcut's keys also reach the app you're typing in.
- Any key or key combination can be the shortcut, shown on a keyboard.

### Improved
- Sentences end with proper punctuation, and the last word is no longer cut off.
- Faster first dictation: the model is warmed up at launch.
- Starting a dictation never blocks the keyboard or the app; the microphone follows headset and input changes.
- Silence and very quiet recordings are no longer turned into made-up text.

### Fixed
- Transcription can be cancelled and stops itself if it takes far too long.
- Dictator checks it may type before you speak, and keeps text it couldn't type in the menu so you can type it again.
- Only one copy of Dictator runs at a time.
- Granting Accessibility now prompts a relaunch so it takes effect.

## [0.0.1] - 2026-10-09

First build: hold or tap a shortcut, speak, and the text is typed where your cursor is. Runs entirely on your Mac with a bundled Whisper model.
