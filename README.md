# FramR

framr is an open-source wayland (wlroots) screenshotting tool with a focus on simplicity and ease of use. It allows users to quickly capture their screen and share the image anywhere using any uploader they'd like.

> [!NOTE]
> framr currently only supports wlroots-based compositors (e.g, sway, hyprland, river, etc) and is not compatible with X11 or non-wlroots compositors (KDE, Gnome). It is also currently in early development and may have some bugs or missing features.

## Features
- Simple and intuitive interface
- Support for multiple uploaders (Any ShareX/iShare compatible uploader)
- Area selection and full-screen capture.

## TODO
- [x] Support multiple monitor screen capture (captureing multiple monitors with the same command.)
- [x] Add default action to config 
- [ ] Add a home-manager module (NixOS)
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
