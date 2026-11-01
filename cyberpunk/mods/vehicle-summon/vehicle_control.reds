module DiffysAttic.VehicleSummon

func Now(game: GameInstance) -> Float {
  return EngineTime.ToFloat(GameInstance.GetSimTime(game));
}

func Flat(v: Vector4) -> Vector4 {
  return new Vector4(v.X, v.Y, 0.0, 0.0);
}

func HorizontalSpeed(vehicle: ref<VehicleObject>) -> Float {
  return Vector4.Length2D(vehicle.GetLinearVelocity());
}

func IsBike(vehicle: ref<VehicleObject>) -> Bool {
  return IsDefined(vehicle as BikeObject);
}

func QuestControlsSummon(game: GameInstance, player: ref<GameObject>) -> Bool {
  if StatusEffectSystem.ObjectHasStatusEffectWithTag(player, n"CustomVehicleSummon") {
    return true;
  }
  return GameInstance.GetQuestsSystem(game).GetFact(n"unlock_car_hud_dpad") == 0;
}

func CancelAllVehicleCommands(vehicle: ref<VehicleObject>) {
  let ai = vehicle.GetAIComponent();
  if !IsDefined(ai) {
    return;
  }
  let attempt = 0;
  while attempt < CancelAttempts() {
    let pending = ai.GetEnqueuedOrExecutingCommand(n"AIVehicleCommand", true);
    if !IsDefined(pending) && !ai.IsCommandExecuting(n"AIVehicleCommand", true) {
      return;
    }
    ai.CancelOrInterruptCommand(n"AIVehicleCommand", true, false);
    if IsDefined(pending) {
      ai.CancelCommand(pending);
    }
    attempt += 1;
  }
}

func CancelCommand(vehicle: ref<VehicleObject>, command: ref<AICommand>) {
  let ai = vehicle.GetAIComponent();
  if IsDefined(ai) && IsDefined(command) {
    ai.CancelCommand(command);
  }
}

func IsVehicleCommandExecuting(vehicle: ref<VehicleObject>) -> Bool {
  let ai = vehicle.GetAIComponent();
  return IsDefined(ai) && ai.IsCommandExecuting(n"AIVehicleCommand", true);
}

func ReleaseBrakes(vehicle: ref<VehicleObject>) {
  vehicle.ForceBrakesUntilStoppedOrFor(0.0);
}

func CanBrake(vehicle: ref<VehicleObject>, isBike: Bool) -> Bool {
  return !isBike || HorizontalSpeed(vehicle) < BikeBrakeMaxSpeed();
}

func StopAndBrake(vehicle: ref<VehicleObject>, isBike: Bool, seconds: Float) {
  CancelAllVehicleCommands(vehicle);
  if CanBrake(vehicle, isBike) {
    vehicle.ForceBrakesUntilStoppedOrFor(seconds);
  }
}

func SetBikeStand(vehicle: ref<VehicleObject>, down: Bool) {
  let evt = new VehicleParkedEvent();
  evt.park = down;
  vehicle.QueueEvent(evt);
}

func SendDriveOrder(vehicle: ref<VehicleObject>, target: Vector4, maxSpeed: Float, minSpeed: Float, startSpeed: Float) -> ref<AICommand> {
  CancelAllVehicleCommands(vehicle);
  vehicle.PhysicsWakeUp();
  let command = new AIVehicleDriveToPointAutonomousCommand();
  command.needDriver = false;
  command.targetPosition = Vector4.Vector4To3(target);
  command.maxSpeed = maxSpeed;
  command.minSpeed = minSpeed;
  command.clearTrafficOnPath = true;
  command.minimumDistanceToTarget = OrderMinDistanceToTarget();
  command.forcedStartSpeed = startSpeed;
  command.driveDownTheRoadIndefinitely = false;
  let evt = new AICommandEvent();
  evt.command = command;
  vehicle.QueueEvent(evt);
  return command;
}

func YawFacing(from: Vector4, to: Vector4) -> Float {
  return Rad2Deg(AtanF(to.Y - from.Y, to.X - from.X)) - 90.0;
}

func TeleportVehicle(vehicle: ref<VehicleObject>, position: Vector4, yaw: Float) {
  CancelAllVehicleCommands(vehicle);
  vehicle.PhysicsWakeUp();
  let rotation: EulerAngles;
  rotation.Yaw = yaw;
  GameInstance.GetTeleportationFacility(vehicle.GetGame()).Teleport(vehicle, position, rotation);
}
