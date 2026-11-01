module DiffysAttic.VehicleSummon

func Raised(point: Vector4, height: Float) -> Vector4 {
  return new Vector4(point.X, point.Y, point.Z + height, 1.0);
}

func StaticRayHits(game: GameInstance, from: Vector4, to: Vector4) -> Bool {
  let hit: TraceResult;
  return GameInstance.GetSpatialQueriesSystem(game).SyncRaycastByCollisionGroup(from, to, n"Static", hit, true, false);
}

func AreStackedLevels(game: GameInstance, a: Vector4, b: Vector4) -> Bool {
  let lower = a;
  let higher = b;
  if a.Z > b.Z {
    lower = b;
    higher = a;
  }
  if higher.Z - lower.Z < StackMinGap() {
    return false;
  }
  let from = new Vector4(lower.X, lower.Y, lower.Z + StackRayAboveLower(), 1.0);
  let to = new Vector4(lower.X, lower.Y, higher.Z - StackRayBelowHigher(), 1.0);
  return StaticRayHits(game, from, to);
}

func IsSeenByPlayer(game: GameInstance, playerPos: Vector4, point: Vector4) -> Bool {
  let screen = GameInstance.GetCameraSystem(game).ProjectPoint(point);
  if screen.Z <= 0.0 {
    return false;
  }
  if AbsF(screen.X) > ScreenMargin() || AbsF(screen.Y) > ScreenMargin() {
    return false;
  }
  return !StaticRayHits(game, Raised(playerPos, EyeHeight()), point);
}

func HasRoomForVehicle(game: GameInstance, ground: Vector4) -> Bool {
  let queries = GameInstance.GetSpatialQueriesSystem(game);
  let center = Raised(ground, RoomCenterHeight());
  let unrotated: EulerAngles;
  let hit: TraceResult;
  if queries.Overlap(RoomHalfExtents(), center, unrotated, n"Static", hit) {
    return false;
  }
  return !queries.Overlap(RoomHalfExtents(), center, unrotated, n"Vehicle", hit);
}

func AcceptSpot(game: GameInstance, playerPos: Vector4, probe: Vector4, out spot: Vector4) -> Bool {
  let found = GameInstance.GetNavigationSystem(game).FindPointInSphereOnlyHumanNavmesh(probe, NavmeshSphereRadius(), NavGenAgentSize.Human, false);
  if NotEquals(found.status, worldNavigationRequestStatus.OK) {
    return false;
  }
  let point = found.point;
  if AbsF(point.Z - playerPos.Z) > LevelTolerance() {
    return false;
  }
  if Vector4.Distance2D(point, playerPos) < SpotMinDistance() {
    return false;
  }
  if IsSeenByPlayer(game, playerPos, Raised(point, VisibilityPointHeight())) {
    return false;
  }
  if !HasRoomForVehicle(game, point) {
    return false;
  }
  spot = point;
  return true;
}

func BehindCameraDirection(game: GameInstance) -> Vector4 {
  let forward = Flat(GameInstance.GetCameraSystem(game).GetActiveCameraForward());
  if Vector4.Length2D(forward) < 0.01 {
    return new Vector4(0.0, -1.0, 0.0, 0.0);
  }
  return Vector4.Normalize2D(-forward);
}

func RotateFlat(dir: Vector4, degrees: Float) -> Vector4 {
  let rad = Deg2Rad(degrees);
  let c = CosF(rad);
  let s = SinF(rad);
  return new Vector4(dir.X * c - dir.Y * s, dir.X * s + dir.Y * c, 0.0, 0.0);
}

func RingOffset(index: Int32) -> Float {
  if index == 0 {
    return 0.0;
  }
  let step = Cast<Float>((index + 1) / 2) * RingAngleStep();
  return index % 2 == 1 ? -step : step;
}

func FindHiddenSpot(game: GameInstance, playerPos: Vector4, vehiclePos: Vector4, aim: Float, out spot: Vector4) -> Bool {
  if Vector4.Distance2D(vehiclePos, playerPos) > DirectProbeMinDistance() {
    let straight = new Vector4(vehiclePos.X, vehiclePos.Y, playerPos.Z, 1.0);
    if AcceptSpot(game, playerPos, straight, spot) {
      return true;
    }
  }
  let radii: array<Float>;
  ArrayPush(radii, aim);
  ArrayPush(radii, aim + RingStep());
  ArrayPush(radii, MaxF(aim - RingStep(), RingMinimum()));
  let behind = BehindCameraDirection(game);
  for radius in radii {
    let i = 0;
    while i < RingDirections() {
      let dir = RotateFlat(behind, RingOffset(i));
      let probe = new Vector4(playerPos.X + dir.X * radius, playerPos.Y + dir.Y * radius, playerPos.Z, 1.0);
      if AcceptSpot(game, playerPos, probe, spot) {
        return true;
      }
      i += 1;
    }
  }
  return false;
}
