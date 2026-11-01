@wrapMethod(GameObject)
protected cb func OnGameAttached() -> Bool {
  let result = wrappedMethod();
  AutoLootSystem.NotifyAttached(this);
  return result;
}

@wrapMethod(GameObject)
protected cb func OnPreUninitialize(evt: ref<entPreUninitializeEvent>) -> Bool {
  AutoLootSystem.NotifyDetached(this);
  return wrappedMethod(evt);
}

@wrapMethod(GameObject)
protected cb func OnDetach() -> Bool {
  AutoLootSystem.NotifyDetached(this);
  return wrappedMethod();
}

@addMethod(ScriptedPuppet)
public final func AutoLootInventoryAccessible() -> Bool {
  return IsDefined(this.m_inventoryComponent) && this.m_inventoryComponent.IsAccessible();
}

@addMethod(gameLootContainerBase)
public final func AutoLootHasPersistentState() -> Bool {
  return IsDefined(this.GetPS());
}

@addMethod(LootContainerObjectAnimatedByTransform)
public final func AutoLootOpenOnce() -> Void {
  if !this.wasOpened {
    this.OpenContainerWithTransformAnimation();
    this.wasOpened = true;
  }
}
