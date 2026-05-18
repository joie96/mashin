namespace mashin.Views.Mobile;

public partial class ExplorePage : ContentPage
{
    public ExplorePage()
    {
        InitializeComponent();
    }

    protected override void OnDisappearing()
    {
        base.OnDisappearing();
        BindingContext = null;
    }
}
