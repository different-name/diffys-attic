module DiffysAttic.VehicleSummon

enum HandlingPhase {
  Settling = 0,
  Driving = 1,
  Parked = 2,
  Finished = 3,
}

enum MoveOutcome {
  InView = 0,
  NoSpot = 1,
  Moved = 2,
}

struct PlayerSample {
  let position: Vector4;
  let velocity: Vector4;
  let speed: Float;
}

struct Leader {
  let found: Bool;
  let position: Vector4;
  let speed: Float;
  let gap: Float;
}

public class SummonedVehicle {
  public let vehicle: wref<VehicleObject>;
  let vehicleID: EntityID;
  let isBike: Bool;
  let isRecall: Bool;
  let phase: HandlingPhase;

  let calledAt: Float;
  let vanillaArrivedAt: Float;

  let hasPlayerSample: Bool;
  let lastPlayerPos: Vector4;
  let lastPlayerSampleAt: Float;
  let playerVelocity: Vector4;
  let playerJumped: Bool;

  let movesUsed: Int32;
  let verifyingTeleport: Bool;
  let teleportedAt: Float;
  let teleportPos: Vector4;
  let teleportYaw: Float;
  let teleportRetried: Bool;
  let levelFixSince: Float;

  let stuckRefPos: Vector4;
  let stuckRefAt: Float;
  let bestDistance: Float;
  let lastProgressAt: Float;

  let lastCommand: ref<AICommand>;
  let lastOrderAt: Float;
  let lastOrderTarget: Vector4;
  let lastOrderMaxSpeed: Float;
  let lastOrderVehiclePos: Vector4;
  let orderRetries: Int32;
  let queued: Bool;
  let queueTargeting: Bool;

  let parkedAt: Float;
  let parkPlayerPos: Vector4;
  let hasParkPos: Bool;
  let stoppedShort: Bool;
  let shortRetries: Int32;
  let gaveUp: Bool;
  let gaveUpPlayerPos: Vector4;

  let standDown: Bool;
  let fallenSince: Float;

  public static func Create(vehicle: ref<VehicleObject>, isRecall: Bool, now: Float) -> ref<SummonedVehicle> {
    let self = new SummonedVehicle();
    self.vehicle = vehicle;
    self.vehicleID = vehicle.GetEntityID();
    self.Restart(isRecall, now);
    return self;
  }

  public func Restart(isRecall: Bool, now: Float) {
    let vehicle: ref<VehicleObject> = this.vehicle;
    if IsDefined(vehicle) && Equals(this.phase, HandlingPhase.Parked) {
      ReleaseBrakes(vehicle);
    }
    let wasStandDown = this.standDown;
    this.isBike = IsDefined(vehicle) && IsBike(vehicle);
    this.isRecall = isRecall;
    this.phase = HandlingPhase.Settling;
    this.calledAt = now;
    this.vanillaArrivedAt = -1.0;
    this.hasPlayerSample = false;
    this.playerVelocity = new Vector4(0.0, 0.0, 0.0, 0.0);
    this.playerJumped = false;
    this.movesUsed = 0;
    this.verifyingTeleport = false;
    this.teleportRetried = false;
    this.levelFixSince = -1.0;
    this.lastCommand = null;
    this.lastOrderAt = -1.0;
    this.orderRetries = 0;
    this.queued = false;
    this.queueTargeting = false;
    this.hasParkPos = false;
    this.stoppedShort = false;
    this.shortRetries = 0;
    this.gaveUp = false;
    this.standDown = this.isBike && (isRecall || wasStandDown);
    this.fallenSince = -1.0;
  }

  public func Matches(vehicle: ref<VehicleObject>) -> Bool {
    return this.vehicleID == vehicle.GetEntityID();
  }

  public func IsFinished() -> Bool {
    return Equals(this.phase, HandlingPhase.Finished);
  }

  public func NoteVanillaArrival(now: Float) {
    if this.IsFinished() {
      return;
    }
    this.vanillaArrivedAt = now;
    if this.isBike {
      this.standDown = true;
    }
  }

  public func ReleaseWithBrake() {
    let vehicle: ref<VehicleObject> = this.vehicle;
    if IsDefined(vehicle) {
      StopAndBrake(vehicle, this.isBike, ReleaseBrakeTime());
    }
    this.phase = HandlingPhase.Finished;
  }

  func ReleaseQuietly() {
    let vehicle: ref<VehicleObject> = this.vehicle;
    if IsDefined(vehicle) {
      CancelCommand(vehicle, this.lastCommand);
      if Equals(this.phase, HandlingPhase.Parked) {
        ReleaseBrakes(vehicle);
      }
    }
    this.lastCommand = null;
    this.phase = HandlingPhase.Finished;
  }

  public func Update(director: ref<SummonDirector>, game: GameInstance, now: Float) {
    if this.IsFinished() {
      return;
    }
    let vehicle: ref<VehicleObject> = this.vehicle;
    let player = GetPlayer(game);
    if !IsDefined(player) || !IsDefined(vehicle) || vehicle.IsDestroyed() {
      this.ReleaseQuietly();
      return;
    }
    if VehicleComponent.IsMountedToVehicle(game, player) {
      if VehicleComponent.IsMountedToProvidedVehicle(game, player.GetEntityID(), vehicle) {
        this.ReleaseQuietly();
      } else {
        this.ReleaseWithBrake();
      }
      return;
    }
    if QuestControlsSummon(game, player) {
      this.ReleaseQuietly();
      return;
    }
    let sample = this.SamplePlayer(player.GetWorldPosition(), now);
    if this.playerJumped || now - this.calledAt > HandlingTimeLimit() {
      this.ReleaseWithBrake();
      return;
    }
    if this.isBike && this.RecoverFallenBike(vehicle, now) {
      return;
    }
    switch this.phase {
      case HandlingPhase.Settling:
        this.UpdateSettling(game, vehicle, sample, now);
        break;
      case HandlingPhase.Driving:
        this.UpdateDriving(director, game, vehicle, sample, now);
        break;
      case HandlingPhase.Parked:
        this.UpdateParked(vehicle, sample, now);
        break;
      default:
        break;
    }
  }

  func SamplePlayer(position: Vector4, now: Float) -> PlayerSample {
    if !this.hasPlayerSample {
      this.hasPlayerSample = true;
      this.lastPlayerPos = position;
      this.lastPlayerSampleAt = now;
    } else {
      let elapsed = now - this.lastPlayerSampleAt;
      if elapsed >= 0.01 {
        let raw = Flat(position - this.lastPlayerPos) / elapsed;
        if Vector4.Length2D(raw) > JumpSpeed() {
          this.playerJumped = true;
          raw = new Vector4(0.0, 0.0, 0.0, 0.0);
        }
        this.playerVelocity = (this.playerVelocity + raw) * 0.5;
        this.lastPlayerPos = position;
        this.lastPlayerSampleAt = now;
      }
    }
    let sample: PlayerSample;
    sample.position = position;
    sample.velocity = this.playerVelocity;
    sample.speed = Vector4.Length2D(this.playerVelocity);
    return sample;
  }

  func RecoverFallenBike(vehicle: ref<VehicleObject>, now: Float) -> Bool {
    let bike = vehicle as BikeObject;
    if !IsDefined(bike) || bike.IsTiltControlEnabled() {
      this.fallenSince = -1.0;
      return false;
    }
    if this.fallenSince < 0.0 {
      CancelAllVehicleCommands(vehicle);
      this.fallenSince = now;
      return true;
    }
    if now - this.fallenSince < FallenRecoveryTime() {
      return true;
    }
    bike.EnableTiltControl(true);
    vehicle.PhysicsWakeUp();
    this.fallenSince = -1.0;
    if Equals(this.phase, HandlingPhase.Driving) {
      this.StartDrive(vehicle, now);
    }
    return true;
  }

  func UpdateSettling(game: GameInstance, vehicle: ref<VehicleObject>, sample: PlayerSample, now: Float) {
    if now - this.calledAt < SettleTime() {
      return;
    }
    let vehiclePos = vehicle.GetWorldPosition();
    if this.isRecall && Vector4.Distance2D(vehiclePos, sample.position) > RecallRelocateDistance() {
      this.TryMove(game, vehicle, sample.position, TakeoverRing(), now);
    } else {
      if AbsF(vehiclePos.Z - sample.position.Z) > LevelTolerance() && AreStackedLevels(game, vehiclePos, sample.position) {
        this.TryMove(game, vehicle, sample.position, TakeoverRing(), now);
      }
    }
    this.StartDrive(vehicle, now);
  }

  func StartDrive(vehicle: ref<VehicleObject>, now: Float) {
    this.phase = HandlingPhase.Driving;
    this.stuckRefPos = vehicle.GetWorldPosition();
    this.stuckRefAt = now;
    this.bestDistance = 1000000.0;
    this.lastProgressAt = now;
    this.lastOrderAt = -1.0;
    this.orderRetries = 0;
    this.queued = false;
    this.queueTargeting = false;
    this.levelFixSince = -1.0;
    if this.isBike && this.standDown {
      SetBikeStand(vehicle, false);
      this.standDown = false;
    }
  }

  func TryMove(game: GameInstance, vehicle: ref<VehicleObject>, playerPos: Vector4, aim: Float, now: Float) -> MoveOutcome {
    let vehiclePos = vehicle.GetWorldPosition();
    if IsSeenByPlayer(game, playerPos, Raised(vehiclePos, VisibilityPointHeight())) {
      return MoveOutcome.InView;
    }
    this.movesUsed += 1;
    let spot: Vector4;
    if !FindHiddenSpot(game, playerPos, vehiclePos, aim, spot) {
      return MoveOutcome.NoSpot;
    }
    this.teleportPos = Raised(spot, TeleportRaise());
    this.teleportYaw = YawFacing(spot, playerPos);
    TeleportVehicle(vehicle, this.teleportPos, this.teleportYaw);
    this.lastCommand = null;
    this.verifyingTeleport = true;
    this.teleportRetried = false;
    this.teleportedAt = now;
    return MoveOutcome.Moved;
  }

  func UpdateDriving(director: ref<SummonDirector>, game: GameInstance, vehicle: ref<VehicleObject>, sample: PlayerSample, now: Float) {
    if this.verifyingTeleport && !this.VerifyTeleport(vehicle, now) {
      return;
    }
    this.UndoVanillaArrival(vehicle, now);
    if this.FixLevelWhileDriving(game, vehicle, sample, now) {
      return;
    }
    if this.HandleStuck(game, vehicle, sample, now) {
      return;
    }
    this.DecideDrive(director, vehicle, sample, now);
  }

  func VerifyTeleport(vehicle: ref<VehicleObject>, now: Float) -> Bool {
    if now - this.teleportedAt < TeleportVerifyDelay() {
      return false;
    }
    if !this.teleportRetried && Vector4.Distance(vehicle.GetWorldPosition(), this.teleportPos) > TeleportTolerance() {
      TeleportVehicle(vehicle, this.teleportPos, this.teleportYaw);
      this.teleportRetried = true;
      this.teleportedAt = now;
      return false;
    }
    this.verifyingTeleport = false;
    this.lastProgressAt = now;
    return true;
  }

  func UndoVanillaArrival(vehicle: ref<VehicleObject>, now: Float) {
    if this.vanillaArrivedAt < 0.0 {
      return;
    }
    if now - this.vanillaArrivedAt > ArrivalUndoWindow() {
      this.vanillaArrivedAt = -1.0;
      return;
    }
    ReleaseBrakes(vehicle);
    if this.lastOrderAt < this.vanillaArrivedAt {
      if this.isBike && this.standDown {
        SetBikeStand(vehicle, false);
        this.standDown = false;
      }
      this.lastOrderAt = -1.0;
    }
  }

  func FixLevelWhileDriving(game: GameInstance, vehicle: ref<VehicleObject>, sample: PlayerSample, now: Float) -> Bool {
    if this.movesUsed >= MovesPerCycle() {
      return false;
    }
    let vehiclePos = vehicle.GetWorldPosition();
    let onOtherLevel = AbsF(vehiclePos.Z - sample.position.Z) > LevelTolerance()
      && Vector4.Distance2D(vehiclePos, sample.position) <= LevelFixRange()
      && AreStackedLevels(game, vehiclePos, sample.position);
    if !onOtherLevel {
      this.levelFixSince = -1.0;
      return false;
    }
    if this.levelFixSince < 0.0 {
      this.levelFixSince = now;
      return false;
    }
    if now - this.levelFixSince < LevelFixHold() {
      return false;
    }
    let outcome = this.TryMove(game, vehicle, sample.position, LevelFixRing(), now);
    if Equals(outcome, MoveOutcome.InView) {
      return false;
    }
    this.levelFixSince = -1.0;
    if Equals(outcome, MoveOutcome.Moved) {
      this.StartDrive(vehicle, now);
      return true;
    }
    return false;
  }

  func HandleStuck(game: GameInstance, vehicle: ref<VehicleObject>, sample: PlayerSample, now: Float) -> Bool {
    let vehiclePos = vehicle.GetWorldPosition();
    if Vector4.Distance2D(vehiclePos, sample.position) < StuckMinDistance() || this.movesUsed >= MovesPerCycle() {
      return false;
    }
    let lost = now - this.lastProgressAt > LostTime() && sample.speed < LostMaxPlayerSpeed();
    if Vector4.Distance(vehiclePos, this.stuckRefPos) > StuckMoveThreshold() {
      this.stuckRefPos = vehiclePos;
      this.stuckRefAt = now;
      if !lost {
        return false;
      }
    } else {
      if now - this.stuckRefAt < StuckTime() && !lost {
        return false;
      }
    }
    let outcome = this.TryMove(game, vehicle, sample.position, StuckRing(), now);
    if Equals(outcome, MoveOutcome.Moved) {
      this.StartDrive(vehicle, now);
    }
    this.stuckRefPos = vehiclePos;
    this.stuckRefAt = now;
    if lost {
      this.lastProgressAt = now - StuckTime();
    }
    return Equals(outcome, MoveOutcome.Moved);
  }

  func BrakingDistance(speed: Float) -> Float {
    let decel = this.isBike ? BikeBrakeDecel() : CarBrakeDecel();
    return speed * speed / (2.0 * decel) + BrakeReaction() * speed;
  }

  func DecideDrive(director: ref<SummonDirector>, vehicle: ref<VehicleObject>, sample: PlayerSample, now: Float) {
    let vehiclePos = vehicle.GetWorldPosition();
    let vehicleVelocity = Flat(vehicle.GetLinearVelocity());
    let vehicleSpeed = Vector4.Length2D(vehicleVelocity);
    let toPlayer = Flat(sample.position - vehiclePos);
    let distance = Vector4.Length2D(toPlayer);
    let toPlayerDir = distance > 0.01 ? Vector4.Normalize2D(toPlayer) : new Vector4(0.0, 0.0, 0.0, 0.0);
    let forward = Vector4.Normalize2D(Flat(vehicle.GetWorldForward()));
    let playerMoving = sample.speed >= MovingPlayerSpeed();
    let running = playerMoving;
    let sameLevel = AbsF(vehiclePos.Z - sample.position.Z) <= LevelTolerance();
    let ahead = Vector4.Dot2D(toPlayer, forward);
    let chasing = running && ahead > ChaseAhead() && Vector4.Dot2D(sample.velocity, forward) > ChaseAlongSpeed();
    let closingSpeed = vehicleSpeed;
    if running {
      closingSpeed = MaxF(0.0, Vector4.Dot2D(vehicleVelocity - sample.velocity, toPlayerDir));
    }

    if distance < this.bestDistance - ProgressStep() || playerMoving {
      this.bestDistance = distance;
      this.lastProgressAt = now;
    }
    let sinceProgress = now - this.lastProgressAt;

    if sameLevel {
      if !playerMoving && distance < OvershootRange() && ahead < -OvershootBehind() && vehicleSpeed > OvershootSpeed() {
        this.Park(vehicle, sample.position, distance, now, false, false);
        return;
      }
      if distance < StopDistance() + StopMargin() || distance - StopDistance() <= this.BrakingDistance(closingSpeed) {
        this.Park(vehicle, sample.position, distance, now, false, false);
        return;
      }
      if running && !chasing && distance < MovingPlayerStopRange() {
        this.Park(vehicle, sample.position, distance, now, false, false);
        return;
      }
      if !chasing && vehicleSpeed > CircleSpeed()
        && ((distance < CircleNearRange() && sinceProgress > CircleNearTime()) || (distance < CircleFarRange() && sinceProgress > CircleFarTime())) {
        this.Park(vehicle, sample.position, distance, now, false, false);
        return;
      }
    }
    if !chasing && distance >= GiveUpMinDistance() && sinceProgress > GiveUpTime() {
      this.Park(vehicle, sample.position, distance, now, true, false);
      return;
    }

    let target = running ? this.LeadPoint(sample, distance, vehicleSpeed) : sample.position;
    let allowed = this.AllowedSpeed(sample, distance, chasing);

    let wasQueueTargeting = this.queueTargeting;
    this.queueTargeting = false;
    let leader = director.FindLeader(this, vehiclePos, forward, sample.position, distance, wasQueueTargeting);
    if leader.found {
      let leaderStopped = leader.speed < StoppedSpeed();
      if leaderStopped && leader.gap <= QueueStopGap() + this.BrakingDistance(vehicleSpeed) {
        this.Park(vehicle, sample.position, distance, now, false, true);
        return;
      }
      if leaderStopped {
        let toLeader = Flat(leader.position - vehiclePos);
        let toLeaderDir = Vector4.Length2D(toLeader) > 0.01 ? Vector4.Normalize2D(toLeader) : forward;
        target = leader.position - toLeaderDir * QueuePointBehind();
        this.queueTargeting = true;
      }
      let margin = leaderStopped ? QueueMarginStopped() : QueueMarginMoving();
      let follow = MaxF(QueueSpeedFloor(), leader.speed + SqrtF(ApproachCurve() * MaxF(0.0, leader.gap - margin)));
      allowed = MinF(allowed, follow);
    }

    if this.ShouldOrder(vehiclePos, vehicleSpeed, target, allowed, distance, now, NotEquals(wasQueueTargeting, this.queueTargeting)) {
      this.Order(vehicle, vehiclePos, vehicleSpeed, target, allowed, now);
    }
  }

  func LeadPoint(sample: PlayerSample, distance: Float, vehicleSpeed: Float) -> Vector4 {
    if sample.speed < LeadMinPlayerSpeed() {
      return sample.position;
    }
    let leadTime = ClampF(distance / MaxF(vehicleSpeed, LeadMinVehicleSpeed()), LeadMinTime(), LeadMaxTime());
    leadTime = MinF(leadTime, LeadMaxDistance() / sample.speed);
    return sample.position + sample.velocity * leadTime;
  }

  func AllowedSpeed(sample: PlayerSample, distance: Float, chasing: Bool) -> Float {
    let top = this.isBike ? MinF(TopSpeed(), BikeTopSpeed()) : TopSpeed();
    let allowed: Float;
    if sample.speed >= MovingPlayerSpeed() && distance < NearRange() {
      let bonus = distance > FollowFarDistance() ? FollowFarBonus() : FollowNearBonus();
      allowed = MinF(MaxF(sample.speed + bonus, FollowFloor()), top);
    } else {
      if distance < NearRange() {
        allowed = MinF(NearSpeed(), top);
      } else {
        if this.movesUsed > 0 {
          allowed = MinF(AfterMoveSpeed(), top);
        } else {
          allowed = top;
        }
      }
    }
    if !chasing {
      let ease = this.isBike ? BikeEaseSpeed() : CarEaseSpeed();
      let curve = SqrtF(ApproachCurve() * MaxF(0.0, distance - StopDistance()));
      allowed = MinF(allowed, MaxF(ease, curve));
    }
    return allowed;
  }

  func ShouldOrder(vehiclePos: Vector4, vehicleSpeed: Float, target: Vector4, allowed: Float, distance: Float, now: Float, queueTargetChanged: Bool) -> Bool {
    if queueTargetChanged || this.lastOrderAt < 0.0 {
      return true;
    }
    let since = now - this.lastOrderAt;
    let targetShift = Vector4.Distance(target, this.lastOrderTarget);
    if since > RetargetInterval() && targetShift > MaxF(RetargetMinShift(), RetargetShiftRatio() * distance) {
      return true;
    }
    if since > SpeedStepInterval() {
      let slowDown = this.lastOrderMaxSpeed - allowed > SpeedDownExcess() && vehicleSpeed > allowed + SpeedDownVehicleExcess();
      let speedUp = allowed - this.lastOrderMaxSpeed > SpeedUpExcess() && since > SpeedUpInterval();
      if slowDown || speedUp {
        return true;
      }
    }
    if since > StalledOrderTime() && vehicleSpeed < StalledOrderSpeed()
      && Vector4.Distance(vehiclePos, this.lastOrderVehiclePos) < StalledOrderRadius()
      && this.orderRetries < StalledOrderRetries() {
      this.orderRetries += 1;
      return true;
    }
    return false;
  }

  func Order(vehicle: ref<VehicleObject>, vehiclePos: Vector4, vehicleSpeed: Float, target: Vector4, allowed: Float, now: Float) {
    let minSpeed = MinF(this.isBike ? BikeMinOrderSpeed() : CarMinOrderSpeed(), allowed);
    let startSpeed: Float;
    if vehicleSpeed > KeepSpeedAbove() {
      startSpeed = MinF(vehicleSpeed, allowed);
    } else {
      startSpeed = MinF(this.isBike ? BikeStartSpeed() : CarStartSpeed(), allowed);
    }
    this.lastCommand = SendDriveOrder(vehicle, target, allowed, minSpeed, startSpeed);
    this.lastOrderAt = now;
    this.lastOrderTarget = target;
    this.lastOrderMaxSpeed = allowed;
    this.lastOrderVehiclePos = vehiclePos;
  }

  func Park(vehicle: ref<VehicleObject>, playerPos: Vector4, distance: Float, now: Float, gaveUp: Bool, queued: Bool) {
    StopAndBrake(vehicle, this.isBike, ParkBrakeTime());
    this.lastCommand = null;
    this.phase = HandlingPhase.Parked;
    this.parkedAt = now;
    this.stoppedShort = distance > StopDistance() + StoppedShortExtra();
    if distance < MoveResetRange() {
      this.movesUsed = 0;
    }
    if this.hasParkPos && Vector4.Distance2D(playerPos, this.parkPlayerPos) > FollowParkShift() {
      this.shortRetries = 0;
    }
    this.parkPlayerPos = playerPos;
    this.hasParkPos = true;
    this.queued = queued;
    this.gaveUp = gaveUp;
    if gaveUp {
      this.gaveUpPlayerPos = playerPos;
    }
  }

  func UpdateParked(vehicle: ref<VehicleObject>, sample: PlayerSample, now: Float) {
    let vehiclePos = vehicle.GetWorldPosition();
    let vehicleSpeed = HorizontalSpeed(vehicle);
    let distance = Vector4.Distance2D(vehiclePos, sample.position);
    let playerShift = Vector4.Distance2D(sample.position, this.parkPlayerPos);
    let stoppedFor = now - this.parkedAt;

    if this.gaveUp && Vector4.Distance2D(sample.position, this.gaveUpPlayerPos) > GiveUpClearDistance() {
      this.gaveUp = false;
    }
    if playerShift > FollowParkShift() {
      this.shortRetries = 0;
    }
    let followDistance = MaxF(FollowMinDistance(), StopDistance() + FollowStopDistanceExtra());
    if !this.gaveUp && distance > followDistance && playerShift > FollowParkShift()
      && stoppedFor > FollowAfter() && vehicleSpeed < StoppedSpeed() {
      ReleaseBrakes(vehicle);
      this.StartDrive(vehicle, now);
      return;
    }
    if this.stoppedShort && !this.queued && !this.gaveUp && this.shortRetries < ShortRetries()
      && distance > StopDistance() + StoppedShortExtra() && stoppedFor > ShortRetryAfter() && vehicleSpeed < StoppedSpeed() {
      this.shortRetries += 1;
      ReleaseBrakes(vehicle);
      this.StartDrive(vehicle, now);
      return;
    }
    if IsVehicleCommandExecuting(vehicle) {
      CancelAllVehicleCommands(vehicle);
    }
    if this.isBike {
      if vehicleSpeed >= BikeBrakeMaxSpeed() {
        return;
      }
      if vehicleSpeed < StandDownSpeed() && stoppedFor > StandDownAfter() && !this.standDown {
        SetBikeStand(vehicle, true);
        this.standDown = true;
      }
    }
    if stoppedFor > RebrakeAfter() && vehicleSpeed > RebrakeSpeed() {
      vehicle.ForceBrakesUntilStoppedOrFor(RebrakeTime());
    } else {
      if stoppedFor > HoldAfter() {
        vehicle.ForceBrakesFor(HoldBrakeTime());
      }
    }
  }
}
