# Native methods are registered against this bridge when the library loads.
-keep class com.example.mashin_flutter.core.RustCoreBridge { *; }

# Rust receives SecureSessionStore from Kotlin, but calls these methods by name.
-keepclassmembers,allowoptimization class com.example.mashin_flutter.persistence.SecureSessionStore {
	public static java.lang.String read();
	public static void write(java.lang.String);
	public static void clear();
}