import share_handler_ios_models

/// Share extension entry point.
///
/// `share_handler_ios` ships `ShareHandlerIosViewController` which handles
/// staging shared items into the app group container and bouncing the user
/// into the main app via the `ShareMedia-<bundleId>` URL scheme declared in
/// Runner/Info.plist. This thin subclass is all the extension needs.
class ShareViewController: ShareHandlerIosViewController {}