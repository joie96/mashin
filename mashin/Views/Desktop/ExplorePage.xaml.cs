using FFImageLoading;

namespace mashin.Views.Desktop;

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