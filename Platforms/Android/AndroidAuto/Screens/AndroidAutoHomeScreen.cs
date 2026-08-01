using AndroidX.Car.App;
using AndroidX.Car.App.Model;
using Action = AndroidX.Car.App.Model.Action;

namespace mashin.Platforms.Android.AndroidAuto.Screens
{
    public class AndroidAutoHomeScreen : Screen
    {
        public AndroidAutoHomeScreen(CarContext carContext) : base(carContext)
        {
        }

        public override ITemplate OnGetTemplate()
        {
            var itemListBuilder = new ItemList.Builder();

            itemListBuilder.AddItem(
                new Row.Builder()
                    .SetTitle("mashin Android Auto")
                    .AddText("Test-Startseite ist aktiv")
                    .Build());

            itemListBuilder.AddItem(
                new Row.Builder()
                    .SetTitle("Verbindung")
                    .AddText("Tippen fur Test-Toast")
                    .SetOnClickListener(new ToastOnClickListener(CarContext, "Android Auto ist verbunden"))
                    .Build());

            itemListBuilder.AddItem(
                new Row.Builder()
                    .SetTitle("Version")
                    .AddText("Erste Integration")
                    .Build());

            return new ListTemplate.Builder()
                .SetHeaderAction(Action.AppIcon)
                .SetTitle("mashin")
                .SetSingleList(itemListBuilder.Build())
                .Build();
        }
    }

    internal sealed class ToastOnClickListener : Java.Lang.Object, IOnClickListener
    {
        private readonly CarContext _carContext;
        private readonly string _message;

        public ToastOnClickListener(CarContext carContext, string message)
        {
            _carContext = carContext;
            _message = message;
        }

        public void OnClick()
        {
            CarToast.MakeText(_carContext, _message, CarToast.LengthShort).Show();
        }
    }
}
