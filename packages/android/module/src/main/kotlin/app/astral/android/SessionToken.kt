package app.astral.android

import android.util.Base64
import org.xmlpull.v1.XmlPullParser
import org.xmlpull.v1.XmlPullParserFactory
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

object SessionToken {
    private const val KEYSTORE = "AndroidKeyStore"
    private const val KEY_ALIAS = "wasm_storage"
    private const val SECURE_STORAGE = "shared_prefs/wasm_secure_storage.xml"
    private const val SECURE_KEY = "shared_prefs/wasm_secure_key.xml"
    private const val VERSIONED_PREFIX = "2:"
    private const val TOKEN_SUFFIX = ":token"
    private const val MULTI_PREFIX = "multi:"
    private const val IV_LENGTH = 12
    private const val TAG_BITS = 128

    fun read(dataDirectory: String): String? {
        val storage = strings(File(dataDirectory, SECURE_STORAGE))
        val encrypted = storage.entries.firstOrNull { (key, _) -> key.endsWith(TOKEN_SUFFIX) && !key.startsWith(MULTI_PREFIX) }?.value
            ?: return null
        val wrappedKey = strings(File(dataDirectory, SECURE_KEY))["data_key"] ?: return null
        val storeKey = keystoreKey() ?: return null
        val dataKey = SecretKeySpec(decrypt(storeKey, Base64.decode(wrappedKey, Base64.NO_WRAP)), "AES")
        val payload = encrypted.removePrefix(VERSIONED_PREFIX)
        val key: SecretKey = if (encrypted.startsWith(VERSIONED_PREFIX)) dataKey else storeKey
        return decrypt(key, Base64.decode(payload, Base64.NO_WRAP)).decodeToString().trim().trim('"')
    }

    private fun keystoreKey(): SecretKey? {
        val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        return (store.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.secretKey
    }

    private fun decrypt(key: SecretKey, sealed: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, sealed, 0, IV_LENGTH))
        return cipher.doFinal(sealed, IV_LENGTH, sealed.size - IV_LENGTH)
    }

    private fun strings(file: File): Map<String, String> {
        if (!file.isFile) return emptyMap()
        val values = mutableMapOf<String, String>()
        val parser = XmlPullParserFactory.newInstance().newPullParser()
        file.inputStream().use { input ->
            parser.setInput(input, "UTF-8")
            var name: String? = null
            while (parser.next() != XmlPullParser.END_DOCUMENT) {
                when (parser.eventType) {
                    XmlPullParser.START_TAG -> name = if (parser.name == "string") parser.getAttributeValue(null, "name") else null
                    XmlPullParser.TEXT -> name?.let { values[it] = parser.text }
                    XmlPullParser.END_TAG -> name = null
                }
            }
        }
        return values
    }
}
