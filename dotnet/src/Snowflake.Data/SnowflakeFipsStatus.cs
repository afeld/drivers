namespace Snowflake.Data;

public readonly struct SnowflakeFipsStatus
{
    internal SnowflakeFipsStatus(bool isFips)
    {
        IsFips = isFips;
    }

    public bool IsFips { get; }
}
