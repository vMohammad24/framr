# FramR

framr is an open-source wayland (wlroots) screenshotting tool with a focus on simplicity and ease of use. It allows users to quickly capture their screen and share the image anywhere using any uploader they'd like.

> [!NOTE]
> framr currently only supports wlroots-based compositors (e.g, sway, hyprland, river, etc) and is not compatible with X11 or non-wlroots compositors (KDE, Gnome). It is also currently in early development and may have some bugs or missing features.

## Features
- Simple and intuitive interface
- Support for multiple uploaders (Any ShareX/iShare compatible uploader)
- Area selection and full-screen capture.

## Installation

### Binary
Pre-built binaries for Linux (x86_64) are available on the [Releases](https://github.com/vMohammad24/framr/releases) page.

### Arch Linux (AUR)
coming soon

### Cargo
You can install `framr` from source using `cargo`:
```bash
cargo install --path .
```

### Nix
If you are using Nix, you can install `framr` by adding it to your configuration or using `nix profile`:
```bash
nix profile add github:vMohammad24/framr
```

#### Home Manager
`framr` includes a Home Manager module that allows you to manage your configuration directly in Nix.

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
                  error_message = "{json:message}"
                  [ [ "authorization" "your_api_key" ] ];
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

### Prerequisites
If you are building from source or running the binary system, you will need the following dependencies:
- `wayland`
- `libxkbcommon`
- `cairo`
- `dbus`
- `libxcursor`


## TODO
- [x] Support multiple monitor screen capture (captureing multiple monitors with the same command.)
- [x] Add default action to config 
- [x] Add a home-manager module (NixOS)
- [ ] Add notifaction support
- [ ] Implement deeplinks for uploaders (e.g, `framr://[base64 of the sharex config or download link])
- [ ] Replace slurp-rs with custom.
- [ ] Implement recording functionality
- [ ] Support all linux desktop environments (currently only tested on hyprland)

## Contributing
Contributions are welcome! If you have an idea for a new feature or have found a bug, please open an issue or submit a pull request.

## License
framr is licensed under the GNU AGPL-3.0 License. See the LICENSE file for more information.

## Credits
- [wayshot](https://github.com/waycrate/wayshot) inpisration for some code snippets.
