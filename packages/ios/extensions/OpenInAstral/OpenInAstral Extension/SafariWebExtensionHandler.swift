
import SafariServices
import os.log

class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {

    private let browserLoginStages: Set<String> = [
        "handoff-captured",
        "reader-loaded",
        "reader-failed",
        "token-unavailable",
        "handoff-created",
        "handoff-failed"
    ]

    func beginRequest(with context: NSExtensionContext) {
        let request = context.inputItems.first as? NSExtensionItem

        let profile: UUID?
        if #available(iOS 17.0, macOS 14.0, *) {
            profile = request?.userInfo?[SFExtensionProfileKey] as? UUID
        } else {
            profile = request?.userInfo?["profile"] as? UUID
        }

        let message: Any?
        if #available(iOS 15.0, macOS 11.0, *) {
            message = request?.userInfo?[SFExtensionMessageKey]
        } else {
            message = request?.userInfo?["message"]
        }

        let stage = (message as? [String: Any])?["browserLoginStage"] as? String
        let accepted = stage.map(browserLoginStages.contains) ?? false
        if let stage, accepted {
            os_log(.default, "Received browser login stage %@ (profile: %@)", stage, profile?.uuidString ?? "none")
        }

        let response = NSExtensionItem()
        if #available(iOS 15.0, macOS 11.0, *) {
            response.userInfo = [SFExtensionMessageKey: ["accepted": accepted]]
        } else {
            response.userInfo = ["message": ["accepted": accepted]]
        }

        context.completeRequest(returningItems: [ response ], completionHandler: nil)
    }

}
