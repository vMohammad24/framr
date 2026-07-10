<div align="center">

# FramR

Screenshot, annotate, record, upload.

[![AUR](https://img.shields.io/aur/version/framr)](https://aur.archlinux.org/packages/framr)
[![Build](https://github.com/vMohammad24/framr/actions/workflows/build.yml/badge.svg)](https://github.com/vMohammad24/framr/actions/workflows/build.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)

[Demo](#demo) - [Install](#installation) - [Quick start](#quick-start) - [Uploaders](#uploaders--deeplinks)

</div>

FramR is a screenshot and screen-recording CLI for Wayland, written in Rust. Select an area, blur out your secrets, and the link ends up in your clipboard when you're done.

Works on wlroots/scenefx compositors (Hyprland, Sway, River, ...) and KDE Plasma on Wayland. No X11, no GNOME.

## Demo

https://github.com/user-attachments/assets/f2244b75-c37a-4a73-a58d-81b44587577b

Area selection, annotations, recording, and instant uploads.

## Features

**Capture.** Full screen, a specific monitor, or area selection across multiple monitors. Re-shoot the last selected region with `--last`.

**Annotate.** Arrows, text, highlights, circles, blur and pixelate for sensitive info. Undo/redo and custom colors work in the selection overlay itself, no separate editor or program.

**Record.** Run once to start, again to stop, or set a fixed length with `--duration`. Encodes to H264 or AV1, outputs MP4, MKV or WebM. Supports hardware encoding with support for AMF, VAAPI, NVENC, QSV.

**Upload.** Any ShareX (`.sxcu`) or iShare (`.iscu`) compatible host works. Import configs from a file, URL, or a `framr://` deeplink. The URL lands in your clipboard when the upload finishes.

**Script.** CLI with straightforward flags. Pipe files or stdin to `framr upload`. Bind it to a key and boom, it works. Clipboard copy, desktop notifications, upload sounds, PNG/JPEG/WebP output, filenames with patterns, and an interactive config wizard to easily configure everything.

## Quick start

```bash
framr -a                    # select an area, annotate, save
framr -a -c                 # select an area, copy to clipboard
framr -a -c -o ~/Pictures   # select an area, save it there and copy it
framr -l                    # re-shoot the last selected region
framr -s 0                  # shoot monitor 0
framr -r                    # record a selected region (run again to stop)
framr -s 0 -r               # record monitor 0
framr -s 0 -r --duration 10 # record monitor 0 for 10 seconds
framr -a -u                 # select, then upload with your default uploader
framr config                # interactive setup wizard
```

## Installation

### Arch Linux (AUR)

```bash
yay -S framr    # or framr-bin for the pre-built binary
```

### Nix

```bash
nix profile add github:vMohammad24/framr
```

Pre-built binaries exist over at [framr.cachix.org](https://framr.cachix.org):

```nix
nix.settings = {
  extra-substituters = [ "https://framr.cachix.org" ];
  extra-trusted-public-keys = [ "framr.cachix.org-1:Nn6BXpOrE0I1sO89xW8l2WVcf2FD4UqU6PD30sgRLZk=" ];
};
```

#### Home Manager

FramR ships a Home Manager module so your whole setup, uploaders included, all living in your Nix config:

```nix
# flake.nix
{
  inputs.framr.url = "github:vMohammad24/framr";

  outputs = { framr, ... }: {
    homeConfigurations."user" = home-manager.lib.homeManagerConfiguration {
      modules = [
        framr.homeManagerModules.default
        {
          programs.framr = {
            enable = true;
            settings = {
              default_action = "UploadAndCopy";
              default_capture = "Area";
              default_uploader = "nest.rip";
              uploaders = [
                {
                  name = "nest.rip";
                  request_method = "POST";
                  request_url = "https://nest.rip/api/files/upload";
                  body_type = "FormData";
                  file_form_name = "files";
                  output_url = "{json:fileURL}";
                  error_message = "{json:message}";
                  headers = [ [ "authorization" "your_api_key" ] ]; # supports `file:/path/to/file`
                }
              ];
            };
          };
        }
      ];
    };
  };
}
```

### Gentoo (community-maintained)

```bash
eselect repository add roxy-overlay git https://codeberg.org/key/roxy-overlay.git
eselect repository enable roxy-overlay
emaint sync -r roxy-overlay
emerge media-gfx/framr
```

### Cargo

```bash
cargo install --git https://github.com/vMohammad24/framr.git
```

### Pre-built binary

Linux x86_64 binaries, `.deb` and `.rpm` packages on the [Releases](https://github.com/vMohammad24/framr/releases) page.

### Dependencies

Building or running the raw binary needs: `wayland`, `libxkbcommon`, `cairo`, `pango`, `dbus`, `libxcursor`, plus GStreamer (with the pipewire plugin) for recording. Package manager installs pull these in automatically.

## Uploaders & deeplinks

Bring your existing ShareX/iShare host config:

```bash
framr config import my-host.sxcu                  # from a file
framr config import https://example.com/host.sxcu # from a URL
```

Or if you own a host, one-click imports via `framr://` deeplinks: either a direct link to a config or the config itself, base64-encoded.

- `framr://https://example.com/uploader.sxcu`
- `framr://eyJOYW1lIjogIk15IFVwbG9hZGVyIiwgLi4ufQ==`

Register the protocol handler (done automatically on NixOS/Home Manager and AUR installations, required on KDE):

```bash
framr config protocol
```

> [!TIP]
> On KDE Plasma, `framr config protocol` also authorizes FramR so it is able to capture screenshots and recordings. Run it once and re-login (or update your desktop database).

## Contributing

Want to report a bug or implement a new feature? Open an issue or send a pull request.

## License

GNU AGPL-3.0. See the [LICENSE](LICENSE) file.

## Credits

- [wayshot](https://github.com/waycrate/wayshot): inspiration for some code snippets.
