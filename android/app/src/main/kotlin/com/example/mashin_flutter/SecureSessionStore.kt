package com.example.mashin_flutter

import android.content.Context
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey

object SecureSessionStore {
    private const val preferencesName = "mashin_secure_session"
    private const val sessionKey = "music_assistant_session"
    private var applicationContext: Context? = null

    fun initialize(context: Context) {
        applicationContext = context.applicationContext
    }

    fun read(): String? = preferences().getString(sessionKey, null)

    fun write(serializedSession: String) {
        preferences().edit().putString(sessionKey, serializedSession).commit()
    }

    fun clear() {
        preferences().edit().remove(sessionKey).commit()
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