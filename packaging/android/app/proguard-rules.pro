-keep class com.google.androidgamesdk.** { *; }
-repackageclasses
-allowaccessmodification
-keepclassmembers class com.libroxel.app.MainActivity {
    public void setMotor(float);
}
