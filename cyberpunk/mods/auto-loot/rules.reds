public abstract class AutoLootRules {

  public static func GatesOpen(player: ref<PlayerPuppet>, game: GameInstance) -> Bool {
    if !AutoLootConfig.Enabled() || !IsDefined(player) || !player.IsAttached() {
      return false;
    }
    if player.IsDead() || player.IsReplacer() || player.IsVRReplacer() || player.IsJohnnyReplacer() {
      return false;
    }
    let hud = player.GetHudManager();
    if !IsDefined(hud) || hud.IsBraindanceActive() {
      return false;
    }
    let psm = player.GetPlayerStateMachineBlackboard();
    if !IsDefined(psm) || psm.GetBool(GetAllBlackboardDefs().PlayerStateMachine.MountedToVehicle) {
      return false;
    }
    if PlayerPuppet.GetSceneTier(player) > EnumInt(gamePSMHighLevel.SceneTier1) {
      return false;
    }
    if GameInstance.GetWorkspotSystem(game).IsActorInWorkspot(player) {
      return false;
    }
    return AutoLootRules.UiIdle(game);
  }

  private static func UiIdle(game: GameInstance) -> Bool {
    let defs = GetAllBlackboardDefs();
    let boards = GameInstance.GetBlackboardSystem(game);
    if !IsDefined(boards) {
      return false;
    }
    let interactions = boards.Get(defs.UIInteractions);
    if !IsDefined(interactions) {
      return false;
    }
    let hubs = FromVariant<DialogChoiceHubs>(interactions.GetVariant(defs.UIInteractions.DialogChoiceHubs));
    if ArraySize(hubs.choiceHubs) > 0 {
      return false;
    }
    let photoMode = boards.Get(defs.PhotoMode);
    if !IsDefined(photoMode) || photoMode.GetBool(defs.PhotoMode.IsActive) {
      return false;
    }
    let ui = boards.Get(defs.UI_System);
    if !IsDefined(ui) || ui.GetBool(defs.UI_System.IsInMenu) {
      return false;
    }
    return true;
  }

  public static func IsCandidate(object: ref<GameObject>) -> Bool {
    if !IsDefined(object) || object.IsPlayer() || object.IsPlayerStash() {
      return false;
    }
    return IsDefined(object as ScriptedPuppet)
      || IsDefined(object as gameItemDropObject)
      || IsDefined(object as gameContainerObjectBase)
      || IsDefined(object as gameLootBag);
  }

  public static func SourceProtected(source: ref<GameObject>) -> Bool {
    return source.IsQuest() || source.IsPlayerStash() || source.IsAnyClueEnabled();
  }

  public static func SourceOpen(source: ref<GameObject>, player: ref<PlayerPuppet>) -> Bool {
    let puppet = source as ScriptedPuppet;
    if IsDefined(puppet) {
      return !puppet.IsActive() && puppet.AutoLootInventoryAccessible();
    }
    if IsDefined(source as gameItemDropObject) || IsDefined(source as gameLootBag) {
      return true;
    }
    let container = source as gameContainerObjectBase;
    if IsDefined(container) {
      return AutoLootConfig.ContainerPickup()
        && container.AutoLootHasPersistentState()
        && container.IsLogicReady()
        && !container.IsDisabled()
        && !container.IsLocked(player);
    }
    return false;
  }

  public static func HasQuestItem(const items: script_ref<array<wref<gameItemData>>>) -> Bool {
    for item in Deref(items) {
      if IsDefined(item) && item.HasTag(n"Quest") {
        return true;
      }
    }
    return false;
  }

  public static func ItemEligible(item: wref<gameItemData>, game: GameInstance) -> Bool {
    if !IsDefined(item) || item.GetQuantity() <= 0 {
      return false;
    }
    let type = item.GetItemType();
    if item.HasTag(n"HideInUI") && NotEquals(type, gamedataItemType.Gen_MoneyShard) {
      return false;
    }
    if item.HasTag(n"TppHead") || item.HasTag(n"NoLootMappin") {
      return false;
    }
    if !AutoLootRules.HasDisplayName(item, type, game) {
      return false;
    }
    return !AutoLootRules.ItemProtected(item, type);
  }

  private static func HasDisplayName(item: wref<gameItemData>, type: gamedataItemType, game: GameInstance) -> Bool {
    let record = TweakDBInterface.GetItemRecord(ItemID.GetTDBID(item.GetID()));
    if StrLen(UIItemsHelper.GetItemName(record, item)) > 0 {
      return true;
    }
    if Equals(type, gamedataItemType.Gen_Readable)
      && IsDefined(record)
      && IsDefined(record.ItemSecondaryAction())
      && IsDefined(GameInstance.GetJournalManager(game)) {
      return StrLen(UIItemsHelper.GetShardName(record, game)) > 0;
    }
    return false;
  }

  private static func ItemProtected(item: wref<gameItemData>, type: gamedataItemType) -> Bool {
    if item.HasTag(n"Quest") || item.HasTag(n"UnequipBlocked") {
      return true;
    }
    if AutoLootRules.BrokenWeaponKept(item) {
      return true;
    }
    if item.HasTag(n"Shard") && !AutoLootRules.KnownShard(item, type) {
      return true;
    }
    if item.HasTag(n"PermanentFood") || item.HasTag(n"TakeAndEquip") {
      return true;
    }
    if Equals(type, gamedataItemType.Gen_Keycard) || Equals(type, gamedataItemType.Wea_HeavyMachineGun) {
      return true;
    }
    return Equals(type, gamedataItemType.Gen_Readable) && !AutoLootConfig.ReadableShards();
  }

  private static func KnownShard(item: wref<gameItemData>, type: gamedataItemType) -> Bool {
    return Equals(type, gamedataItemType.Gen_Readable)
      || Equals(type, gamedataItemType.Con_Skillbook)
      || Equals(type, gamedataItemType.Gen_MoneyShard)
      || item.HasTag(n"Recipe");
  }

  // the player's item-added handler disassembles broken weapons, except where crafting refuses to
  private static func BrokenWeaponKept(item: wref<gameItemData>) -> Bool {
    let id = item.GetID();
    if !RPGManager.IsItemWeapon(id) || !RPGManager.IsItemBroken(item) {
      return false;
    }
    return item.HasTag(n"IconicWeapon") || !IsDefined(ItemActionsHelper.GetDisassembleAction(id));
  }

  public static func GatherObjectivePositions(game: GameInstance, out positions: array<Vector4>) -> Bool {
    let journal = GameInstance.GetJournalManager(game);
    let mappins = GameInstance.GetMappinSystem(game);
    if !IsDefined(journal) || !IsDefined(mappins) {
      return false;
    }
    let filter: JournalRequestStateFilter;
    filter.active = true;
    let context: JournalRequestContext;
    context.stateFilter = filter;
    let quests: array<wref<JournalEntry>>;
    journal.GetQuests(context, quests);
    for quest in quests {
      let entries: array<wref<JournalEntry>>;
      QuestLogUtils.UnpackRecursiveWithFilter(journal, quest as JournalContainerEntry, filter, entries);
      for entry in entries {
        let objective = entry as JournalQuestObjective;
        if IsDefined(objective) {
          let mappin = mappins.GetMappinFromObjective(journal.GetParentEntry(objective), objective);
          if IsDefined(mappin) {
            ArrayPush(positions, mappin.GetWorldPosition());
          }
        }
      }
    }
    return true;
  }
}
