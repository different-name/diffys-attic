module DiffysAttic.VehicleSummon

func StopDistance() -> Float = 4.5
func LevelTolerance() -> Float = 4.0
func TopSpeed() -> Float = 20.0
func BikeTopSpeed() -> Float = 18.0

func TickInterval() -> Float = 0.1
func MaxHandled() -> Int32 = 3
func HandlingTimeLimit() -> Float = 600.0
func MovesPerCycle() -> Int32 = 3
func JumpSpeed() -> Float = 60.0
func SettleTime() -> Float = 0.4
func RecallRelocateDistance() -> Float = 100.0

func StackMinGap() -> Float = 2.8
func StackRayAboveLower() -> Float = 2.2
func StackRayBelowHigher() -> Float = 0.3

func LevelFixRange() -> Float = 45.0
func LevelFixHold() -> Float = 2.0

func StuckMinDistance() -> Float = 35.0
func StuckMoveThreshold() -> Float = 2.0
func StuckTime() -> Float = 6.0
func LostTime() -> Float = 12.0
func LostMaxPlayerSpeed() -> Float = 2.0

func TakeoverRing() -> Float = 45.0
func LevelFixRing() -> Float = 35.0
func StuckRing() -> Float = 30.0
func RingStep() -> Float = 15.0
func RingMinimum() -> Float = 18.0
func RingDirections() -> Int32 = 12
func RingAngleStep() -> Float = 30.0
func NavmeshSphereRadius() -> Float = 6.0
func SpotMinDistance() -> Float = 15.0
func DirectProbeMinDistance() -> Float = 15.0
func RoomHalfExtents() -> Vector4 = new Vector4(2.0, 3.0, 0.8, 0.0)
func RoomCenterHeight() -> Float = 1.4
func TeleportRaise() -> Float = 0.3
func VisibilityPointHeight() -> Float = 1.0
func EyeHeight() -> Float = 1.7
func ScreenMargin() -> Float = 1.1

func TeleportVerifyDelay() -> Float = 0.5
func TeleportTolerance() -> Float = 4.0
func ArrivalUndoWindow() -> Float = 1.5

func RetargetInterval() -> Float = 4.0
func RetargetMinShift() -> Float = 8.0
func RetargetShiftRatio() -> Float = 0.3
func SpeedStepInterval() -> Float = 0.8
func SpeedDownExcess() -> Float = 1.5
func SpeedDownVehicleExcess() -> Float = 0.5
func SpeedUpExcess() -> Float = 3.0
func SpeedUpInterval() -> Float = 2.0
func StalledOrderTime() -> Float = 3.0
func StalledOrderSpeed() -> Float = 0.5
func StalledOrderRadius() -> Float = 1.0
func StalledOrderRetries() -> Int32 = 2

func OrderMinDistanceToTarget() -> Float = 10.0
func CarMinOrderSpeed() -> Float = 4.0
func BikeMinOrderSpeed() -> Float = 2.0
func CarStartSpeed() -> Float = 6.0
func BikeStartSpeed() -> Float = 3.0
func KeepSpeedAbove() -> Float = 2.0

func NearRange() -> Float = 50.0
func NearSpeed() -> Float = 11.0
func AfterMoveSpeed() -> Float = 15.0
func FollowFarDistance() -> Float = 20.0
func FollowFarBonus() -> Float = 5.0
func FollowNearBonus() -> Float = 1.5
func FollowFloor() -> Float = 6.0
func ApproachCurve() -> Float = 5.0
func CarEaseSpeed() -> Float = 5.0
func BikeEaseSpeed() -> Float = 4.5

func CarBrakeDecel() -> Float = 12.0
func BikeBrakeDecel() -> Float = 8.0
func BrakeReaction() -> Float = 0.15

func LeadMinPlayerSpeed() -> Float = 1.5
func LeadMinTime() -> Float = 0.5
func LeadMaxTime() -> Float = 4.0
func LeadMinVehicleSpeed() -> Float = 6.0
func LeadMaxDistance() -> Float = 20.0

func MovingPlayerSpeed() -> Float = 2.0
func ChaseAhead() -> Float = 2.0
func ChaseAlongSpeed() -> Float = 1.5

func OvershootRange() -> Float = 15.0
func OvershootBehind() -> Float = 2.0
func OvershootSpeed() -> Float = 1.0
func StopMargin() -> Float = 3.0
func MovingPlayerStopRange() -> Float = 12.0
func CircleSpeed() -> Float = 2.0
func CircleNearRange() -> Float = 15.0
func CircleNearTime() -> Float = 3.0
func CircleFarRange() -> Float = 30.0
func CircleFarTime() -> Float = 8.0
func GiveUpMinDistance() -> Float = 20.0
func GiveUpTime() -> Float = 20.0
func ProgressStep() -> Float = 0.5

func LaneMinAhead() -> Float = 2.0
func LaneMaxAhead() -> Float = 45.0
func LaneHalfWidth() -> Float = 3.0
func CorridorHalfWidth() -> Float = 3.5
func CorridorHalfWidthQueued() -> Float = 5.5
func StoppedByPlayerRange() -> Float = 10.0
func StoppedSpeed() -> Float = 1.0
func StoppedByPlayerSlack() -> Float = 1.0
func BumperAllowance() -> Float = 5.0
func QueueStopGap() -> Float = 2.5
func QueuePointBehind() -> Float = 7.0
func QueueMarginStopped() -> Float = 2.5
func QueueMarginMoving() -> Float = 6.0
func QueueSpeedFloor() -> Float = 3.0

func ParkBrakeTime() -> Float = 3.0
func HoldAfter() -> Float = 0.5
func HoldBrakeTime() -> Float = 0.3
func RebrakeAfter() -> Float = 3.0
func RebrakeSpeed() -> Float = 4.0
func RebrakeTime() -> Float = 2.0
func FollowMinDistance() -> Float = 15.0
func FollowStopDistanceExtra() -> Float = 10.0
func FollowParkShift() -> Float = 8.0
func FollowAfter() -> Float = 1.0
func StoppedShortExtra() -> Float = 8.0
func ShortRetries() -> Int32 = 2
func ShortRetryAfter() -> Float = 3.0
func GiveUpClearDistance() -> Float = 10.0
func MoveResetRange() -> Float = 12.0

func BikeBrakeMaxSpeed() -> Float = 4.5
func StandDownSpeed() -> Float = 0.5
func StandDownAfter() -> Float = 1.0
func FallenRecoveryTime() -> Float = 1.0

func ReleaseBrakeTime() -> Float = 4.0
func CancelAttempts() -> Int32 = 5
