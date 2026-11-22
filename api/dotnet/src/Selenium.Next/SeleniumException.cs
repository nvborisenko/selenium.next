using System;

namespace Selenium.Next;

public class SeleniumException : Exception
{
    public SeleniumErrorKind Kind { get; }

    public SeleniumException(SeleniumErrorKind kind, string message) : base($"{kind}: {message}") => Kind = kind;
}
