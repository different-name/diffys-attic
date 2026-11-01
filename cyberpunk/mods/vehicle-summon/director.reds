module DiffysAttic.VehicleSummon

public class SummonTick extends DelayCallback {
  let director: wref<SummonDirector>;

  public func Call() {
    let director = this.director;
    if IsDefined(director) {
      director.Tick();
    }
  }
}

public class SummonDirector extends ScriptableSystem {
  let handled: array<ref<SummonedVehicle>>;
  let ticking: Bool;
  let tickID: DelayID;
  let detached: Bool;

  public static func Get(game: GameInstance) -> ref<SummonDirector> {
    return GameInstance.GetScriptableSystemsContainer(game).Get(n"DiffysAttic.VehicleSummon.SummonDirector") as SummonDirector;
  }

  private func OnAttach() {
    this.detached = false;
  }

  private func OnDetach() {
    this.detached = true;
    if this.ticking {
      GameInstance.GetDelaySystem(this.GetGameInstance()).CancelCallback(this.tickID);
      this.ticking = false;
    }
    ArrayClear(this.handled);
  }

  public func OnSummonStarted(vehicle: ref<VehicleObject>, state: vehicleSummonState) {
    if NotEquals(state, vehicleSummonState.EnRoute) && NotEquals(state, vehicleSummonState.AlreadySummoned) {
      return;
    }
    let game = this.GetGameInstance();
    let player = GetPlayer(game);
    if !IsDefined(player) || QuestControlsSummon(game, player) {
      return;
    }
    let isRecall = Equals(state, vehicleSummonState.AlreadySummoned);
    let now = Now(game);
    let existing = this.Find(vehicle);
    if IsDefined(existing) {
      existing.Restart(isRecall, now);
    } else {
      if ArraySize(this.handled) >= MaxHandled() {
        this.handled[0].ReleaseWithBrake();
        ArrayErase(this.handled, 0);
      }
      ArrayPush(this.handled, SummonedVehicle.Create(vehicle, isRecall, now));
    }
    this.ScheduleTick();
  }

  public func OnVanillaArrival(vehicle: ref<VehicleObject>) {
    let entry = this.Find(vehicle);
    if IsDefined(entry) {
      entry.NoteVanillaArrival(Now(this.GetGameInstance()));
    }
  }

  func Find(vehicle: ref<VehicleObject>) -> ref<SummonedVehicle> {
    for entry in this.handled {
      if entry.Matches(vehicle) {
        return entry;
      }
    }
    return null;
  }

  func ScheduleTick() {
    if this.ticking || this.detached {
      return;
    }
    let callback = new SummonTick();
    callback.director = this;
    this.tickID = GameInstance.GetDelaySystem(this.GetGameInstance()).DelayCallback(callback, TickInterval(), false);
    this.ticking = true;
  }

  public func Tick() {
    this.ticking = false;
    if this.detached {
      return;
    }
    let game = this.GetGameInstance();
    let now = Now(game);
    for entry in this.handled {
      entry.Update(this, game, now);
    }
    let remaining: array<ref<SummonedVehicle>>;
    for entry in this.handled {
      if !entry.IsFinished() {
        ArrayPush(remaining, entry);
      }
    }
    this.handled = remaining;
    if ArraySize(this.handled) > 0 {
      this.ScheduleTick();
    }
  }

  public func FindLeader(follower: ref<SummonedVehicle>, followerPos: Vector4, forward: Vector4, playerPos: Vector4, ownDistance: Float, wasQueueTargeting: Bool) -> Leader {
    let leader: Leader;
    let best = 1000000.0;
    let toPlayer = Flat(playerPos - followerPos);
    let toPlayerDir = ownDistance > 0.01 ? Vector4.Normalize2D(toPlayer) : forward;
    let corridor = wasQueueTargeting ? CorridorHalfWidthQueued() : CorridorHalfWidth();
    for other in this.handled {
      let otherVehicle: ref<VehicleObject> = other.vehicle;
      if other != follower && !other.IsFinished() && IsDefined(otherVehicle) {
        let otherPos = otherVehicle.GetWorldPosition();
        let otherToPlayer = Vector4.Distance2D(otherPos, playerPos);
        let otherSpeed = HorizontalSpeed(otherVehicle);
        if otherToPlayer < ownDistance {
          let measure = QueueMeasure(Flat(otherPos - followerPos), otherSpeed, otherToPlayer, forward, toPlayerDir, ownDistance, corridor);
          if measure >= 0.0 && measure < best {
            best = measure;
            leader.found = true;
            leader.position = otherPos;
            leader.speed = otherSpeed;
            leader.gap = measure - BumperAllowance();
          }
        }
      }
    }
    return leader;
  }
}

func QueueMeasure(offset: Vector4, otherSpeed: Float, otherToPlayer: Float, forward: Vector4, toPlayerDir: Vector4, ownDistance: Float, corridor: Float) -> Float {
  let alongLane = Vector4.Dot2D(offset, forward);
  let offLane = AbsF(offset.X * forward.Y - offset.Y * forward.X);
  if alongLane >= LaneMinAhead() && alongLane <= LaneMaxAhead() && offLane < LaneHalfWidth() {
    return alongLane;
  }
  let alongPath = Vector4.Dot2D(offset, toPlayerDir);
  let offPath = AbsF(offset.X * toPlayerDir.Y - offset.Y * toPlayerDir.X);
  if alongPath >= LaneMinAhead() && alongPath <= MinF(LaneMaxAhead(), ownDistance) && offPath < corridor {
    return alongPath;
  }
  let stoppedByPlayer = otherSpeed < StoppedSpeed() && otherToPlayer < StoppedByPlayerRange();
  if stoppedByPlayer && alongPath >= LaneMinAhead() && alongPath <= ownDistance + StoppedByPlayerSlack() {
    return alongPath;
  }
  return -1.0;
}
