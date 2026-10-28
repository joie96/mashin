package com.joie96.mashin.persistence

import android.content.Context
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import com.joie96.mashin.core.RustCoreBridge

object SecureSessionStore {
    private const val preferencesName = "mashin_secure_session"
    private const val sessionKey = "music_assistant_session"
    private var applicationContext: Context? = null

    // RustCoreBridge initializes this store before Rust can read or mutate the persisted session.
    fun initialize(context: Context) {
        applicationContext = context.applicationContext
    }

    @JvmStatic
    fun read(): String? = preferences().getString(sessionKey, null)

    @JvmStatic
    fun write(serializedSession: String) {
        check(preferences().edit().putString(sessionKey, serializedSession).commit()) {
            "Writing secure session credentials failed"
        }
    }

    @JvmStatic
    fun clear() {
        check(preferences().edit().remove(sessionKey).commit()) {
            "Clearing secure session credentials failed"
        }
    }

    private fun preferences() = EncryptedSharedPreferences.create(
        applicationContext ?: error("SecureSessionStore is not initialized"),
        preferencesName,
        MasterKey.Builder(applicationContext ?: error("SecureSessionStore is not initialized"))
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build(),
        EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
        EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
    )
}