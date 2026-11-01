module DiffysAttic.VehicleSummon

@wrapMethod(VehicleComponent)
protected cb func OnSummonStartedEvent(evt: ref<SummonStartedEvent>) -> Bool {
  let result = wrappedMethod(evt);
  let vehicle: ref<VehicleObject> = this.GetVehicle();
  if IsDefined(vehicle) {
    let director = SummonDirector.Get(vehicle.GetGame());
    if IsDefined(director) {
      director.OnSummonStarted(vehicle, evt.state);
    }
  }
  return result;
}

@wrapMethod(VehicleComponent)
protected cb func OnSummonFinishedEvent(evt: ref<SummonFinishedEvent>) -> Bool {
  let result = wrappedMethod(evt);
  if Equals(evt.state, vehicleSummonState.Arrived) {
    let vehicle: ref<VehicleObject> = this.GetVehicle();
    if IsDefined(vehicle) {
      let director = SummonDirector.Get(vehicle.GetGame());
      if IsDefined(director) {
        director.OnVanillaArrival(vehicle);
      }
    }
  }
  return result;
}
