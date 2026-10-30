{
  mkShell,
  androidenv,
  gradle,
  jdk17,
  kotlin-language-server,
}:

let
  buildTools = "36.0.0";
  sdk =
    (androidenv.composeAndroidPackages {
      platformVersions = [ "36" ];
      buildToolsVersions = [ buildTools ];
    }).androidsdk;
  home = "${sdk}/libexec/android-sdk";
in
mkShell {
  packages = [
    sdk
    gradle
    jdk17
    kotlin-language-server
  ];

  ANDROID_HOME = home;
  JAVA_HOME = jdk17.home;
  # the aapt2 gradle downloads from maven is dynamically linked and won't run on nixos
  GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${home}/build-tools/${buildTools}/aapt2";
}
