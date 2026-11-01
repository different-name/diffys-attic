public abstract class AutoLootConfig {

  public static func Enabled() -> Bool {
    return true;
  }

  public static func PickupRadius() -> Float {
    return 5.0;
  }

  public static func ContainerPickup() -> Bool {
    return true;
  }

  public static func PickupSound() -> Bool {
    return true;
  }

  public static func ReadableShards() -> Bool {
    return true;
  }

  public static func ScanInterval() -> Float {
    return 0.15;
  }

  public static func MaxSourcesPerPass() -> Int32 {
    return 64;
  }

  public static func MaxTransfersPerPass() -> Int32 {
    return 16;
  }

  public static func ObjectiveExclusionDistance() -> Float {
    return 3.0;
  }

  public static func EffectiveRadius() -> Float {
    let radius = AutoLootConfig.PickupRadius();
    if radius != radius || radius < 1.0 || radius > 10.0 {
      return 5.0;
    }
    return radius;
  }
}
